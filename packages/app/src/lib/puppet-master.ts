import { formatPaneListForOrchestrator, isOrchestratorPaneId, type LlmModel, type PaneStatus } from '@puppet-master/shared';
import type { ChatMessage, LlmResponse } from './llm';
import { streamLlm } from './llm';
import { executeMcpTool, loadPuppetMasterTools } from './mcp-tools';
import type { McpToolExecutor } from './mcp-executor';
import type { WaitForPanesResult } from './bridge';
import { sleep } from './ansi';
import { SYSTEM_PROMPT } from './puppet-master-prompt';
import { approvePermissionIfPresent } from './tui-autopilot';

export interface PuppetMasterCallbacks {
  onAssistantText: (text: string) => void;
  onToolCall: (tool: string, args: unknown, result?: string, error?: string) => void;
  onComplete: () => void;
  onError: (err: Error) => void;
  /** Emitted periodically while the orchestrator stands idle waiting for worker panes. */
  onStandby?: (running: Array<{ id: string; status: PaneStatus }>) => void;
  /** Emitted when the harness auto-approves a permission prompt on a worker pane. */
  onAutoApprove?: (paneId: string) => void;
}

const MAX_TURNS = 20;
const STANDBY_POLL_MS = 1500;
const STANDBY_MAX_MS = 10 * 60_000;

const SPAWN_PANE_ID_RE = /(?:spawned pane|reusing existing pane):\s([^\s(]+)/;

/** Extract the pane id from a spawn_agent tool result string. */
export function parseSpawnedPaneId(result: string): string | null {
  const m = result.match(SPAWN_PANE_ID_RE);
  return m ? (m[1] ?? null) : null;
}

export type StandbyReason =
  | { reason: 'aborted' }
  | { reason: 'all_settled'; autoApproved: string[] }
  | { reason: 'timeout'; lastStatuses: Array<{ id: string; status: PaneStatus | 'gone' }> }
  | { reason: 'changed'; notes: string[]; autoApproved: string[] };

function describeStatus(status: PaneStatus | 'gone'): string {
  switch (status) {
    case 'waiting_input':
      return 'is waiting for input';
    case 'error':
      return 'errored';
    case 'gone':
      return 'exited (no longer listed)';
    case 'idle':
      return 'is idle';
    default:
      return `is ${status}`;
  }
}

async function processWaitResult(
  executor: McpToolExecutor,
  wait: WaitForPanesResult,
  tracked: Set<string>,
  previousStatus: Map<string, PaneStatus | 'gone'>,
  signal: AbortSignal,
  onAutoApprove?: (paneId: string) => void,
): Promise<{ notes: string[]; running: number; autoApproved: string[] }> {
  const autoApproved: string[] = [];
  if (wait.reason === 'timeout') {
    const panes = await executor.listPanes();
    let running = 0;
    for (const id of tracked) {
      const pane = panes.find((p) => p.id === id);
      const current: PaneStatus | 'gone' = pane ? pane.status : 'gone';
      previousStatus.set(id, current);
      if (current === 'running') running++;
    }
    return { notes: [], running, autoApproved };
  }

  const paneId = wait.pane_id;
  const current = (wait.status ?? 'gone') as PaneStatus | 'gone';
  const prev = previousStatus.get(paneId);
  const notes: string[] = [];

  if (wait.reason === 'permission' && executor.replyOpencodePermission) {
    const permId = wait.opencode?.pending_permission_ids[0];
    if (permId) {
      try {
        await executor.replyOpencodePermission(paneId, permId, 'once');
        autoApproved.push(paneId);
        onAutoApprove?.(paneId);
      } catch {
        notes.push(`pane ${paneId} has opencode permission (auto-approve failed)`);
      }
    }
  } else if (wait.reason === 'key_swap_required') {
    const profiles = wait.from_profile
      ? ` (profile ${wait.from_profile})`
      : '';
    notes.push(`pane ${paneId} hit OpenCode rate limit${profiles} — call rotate_opencode_key`);
  } else if (wait.reason === 'key_rotated') {
    const from = wait.from_profile ?? '?';
    const to = wait.to_profile ?? '?';
    notes.push(`pane ${paneId} auto-rotated OpenCode key ${from} → ${to}`);
  } else if (current !== prev && current !== 'running') {
    if (current === 'waiting_input') {
      const verdict = await approvePermissionIfPresent(executor, paneId, signal);
      if (verdict === 'aborted') return { notes: [], running: 0, autoApproved };
      if (verdict === 'approved') {
        autoApproved.push(paneId);
        onAutoApprove?.(paneId);
      } else {
        notes.push(`pane ${paneId} ${describeStatus(current)}`);
      }
    } else {
      notes.push(`pane ${paneId} ${describeStatus(current)}`);
    }
  }

  const panes = await executor.listPanes();
  let running = 0;
  for (const id of tracked) {
    const pane = panes.find((p) => p.id === id);
    const status: PaneStatus | 'gone' = pane ? pane.status : 'gone';
    previousStatus.set(id, status);
    if (status === 'running') running++;
  }
  return { notes, running, autoApproved };
}

/**
 * Stand idle until tracked worker panes settle (finish, need input, error, or
 * exit), or until the user aborts, or until the standby budget is exhausted.
 *
 * When a tracked pane transitions to waiting_input, the harness first tries to
 * auto-approve a routine permission prompt. If the buffer looks like a
 * permission/approval prompt, the harness sends the approval keystroke and
 * continues standby (the LLM is NOT woken). Only if the buffer does NOT look
 * like a permission prompt — i.e., the worker needs substantive input — is the
 * LLM woken with a status update.
 *
 * `previousStatus` carries the LLM's last-known status for each pane across
 * successive standby calls so we only wake the LLM on actual transitions.
 */
export async function standIdleForWorkers(
  executor: McpToolExecutor,
  tracked: Set<string>,
  previousStatus: Map<string, PaneStatus | 'gone'>,
  signal: AbortSignal,
  onStandby?: (running: Array<{ id: string; status: PaneStatus }>) => void,
  onAutoApprove?: (paneId: string) => void,
  opts: { pollMs?: number; maxMs?: number; now?: () => number } = {},
): Promise<StandbyReason> {
  const pollMs = opts.pollMs ?? STANDBY_POLL_MS;
  const maxMs = opts.maxMs ?? STANDBY_MAX_MS;
  const now = opts.now ?? Date.now;
  if (tracked.size === 0) return { reason: 'all_settled', autoApproved: [] };

  const emitStandby = (statuses: Map<string, PaneStatus | 'gone'>) => {
    if (!onStandby) return;
    const running: Array<{ id: string; status: PaneStatus }> = [];
    for (const [id, s] of statuses) {
      if (s === 'running') running.push({ id, status: 'running' });
    }
    onStandby(running);
  };

  const pollOnce = async (): Promise<{ notes: string[]; running: number; autoApproved: string[] }> => {
    const panes = await executor.listPanes();
    const notes: string[] = [];
    const autoApproved: string[] = [];
    let running = 0;
    for (const id of tracked) {
      const pane = panes.find((p) => p.id === id);
      const current: PaneStatus | 'gone' = pane ? pane.status : 'gone';
      const prev = previousStatus.get(id);
      if (current === 'running') running++;
      // Notify on any transition into a non-running state (including the
      // first observation of a non-running pane). Transitions into 'running'
      // (e.g. worker resumed after input) are silent — just keep waiting.
      if (current !== prev && current !== 'running') {
        if (current === 'waiting_input') {
          // Try to auto-approve a routine permission prompt before waking the
          // LLM. If the buffer looks like an approval dialog, send the
          // approval keystroke and continue standby without waking the LLM.
          const verdict = await approvePermissionIfPresent(executor, id, signal);
          if (verdict === 'aborted') return { notes, running, autoApproved };
          if (verdict === 'approved') {
            autoApproved.push(id);
            onAutoApprove?.(id);
            // Don't add a note — we handled it. The pane should transition
            // back to 'running' on the next poll once the worker resumes.
            previousStatus.set(id, current);
            continue;
          }
          // 'not-prompted' — substantive input needed. Wake the LLM.
        }
        notes.push(`pane ${id} ${describeStatus(current)}`);
      }
      previousStatus.set(id, current);
    }
    return { notes, running, autoApproved };
  };

  const seed = await pollOnce();
  if (signal.aborted) return { reason: 'aborted' };
  emitStandby(previousStatus);
  if (seed.notes.length > 0) return { reason: 'changed', notes: seed.notes, autoApproved: seed.autoApproved };
  if (seed.running === 0) return { reason: 'all_settled', autoApproved: seed.autoApproved };

  const deadline = now() + maxMs;
  while (true) {
    if (signal.aborted) return { reason: 'aborted' };

    const poll = executor.waitForPanes
      ? await processWaitResult(
          executor,
          await executor.waitForPanes({
            pane_ids: [...tracked],
            timeout_ms: Math.max(1_000, Math.min(pollMs * 4, deadline - now())),
          }),
          tracked,
          previousStatus,
          signal,
          onAutoApprove,
        )
      : await (async () => {
          await sleep(pollMs, signal);
          if (signal.aborted) return { notes: [] as string[], running: 0, autoApproved: [] as string[] };
          return pollOnce();
        })();
    if (signal.aborted) return { reason: 'aborted' };
    emitStandby(previousStatus);

    if (poll.notes.length > 0) return { reason: 'changed', notes: poll.notes, autoApproved: poll.autoApproved };
    if (poll.running === 0) return { reason: 'all_settled', autoApproved: poll.autoApproved };
    if (now() >= deadline) {
      return {
        reason: 'timeout',
        lastStatuses: [...previousStatus.entries()].map(([id, status]) => ({ id, status })),
      };
    }
  }
}

export async function runPuppetMasterLoop(
  model: LlmModel,
  apiKey: string,
  executor: McpToolExecutor,
  history: ChatMessage[],
  userPrompt: string,
  cb: PuppetMasterCallbacks,
  signal: AbortSignal,
): Promise<void> {
  const tools = await loadPuppetMasterTools(executor);
  const livePanes = await executor.listPanes();
  const paneSnapshot =
    livePanes.length > 0
      ? `\n\n[Current open panes — reuse these; do not spawn duplicates]\n${formatPaneListForOrchestrator(livePanes)}`
      : '';

  const messages: ChatMessage[] = [
    ...history,
    { role: 'user', content: userPrompt + paneSnapshot },
  ];
  let turns = 0;

  // Worker panes the LLM has spawned or written to this run. The orchestrator
  // stands idle until these settle (or the user aborts).
  const trackedWorkerPaneIds = new Set<string>();
  // Carries the LLM's last-known status for each tracked pane across standby
  // calls so we only wake the LLM on actual state transitions.
  const previousStatus = new Map<string, PaneStatus | 'gone'>();

  const trackFromToolCall = (name: string, args: Record<string, unknown>, result: string) => {
    if (name === 'spawn_agent') {
      const id = (args.pane_id as string | undefined) ?? parseSpawnedPaneId(result);
      if (id && !isOrchestratorPaneId(id)) trackedWorkerPaneIds.add(id);
    } else if (name === 'write_terminal_input' || name === 'press_key') {
      const id = args.pane_id as string | undefined;
      if (id && !isOrchestratorPaneId(id)) trackedWorkerPaneIds.add(id);
    } else if (name === 'kill_pane_process') {
      const id = args.pane_id as string | undefined;
      if (id) {
        trackedWorkerPaneIds.delete(id);
        previousStatus.delete(id);
      }
    }
  };

  while (turns < MAX_TURNS) {
    if (signal.aborted) return;
    turns++;

    let resp: LlmResponse;
    try {
      resp = await streamLlm(
        model.provider,
        apiKey,
        {
          model: model.model_id,
          system: SYSTEM_PROMPT,
          messages,
          tools,
        },
        {
          onTextDelta: (t) => cb.onAssistantText(t),
          signal,
        },
      );
    } catch (err) {
      cb.onError(err instanceof Error ? err : new Error(String(err)));
      return;
    }

    messages.push({ role: 'assistant', content: resp.content, stop_reason: resp.stop_reason });

    // If no tool calls, the LLM is yielding. Check whether worker panes are
    // still running before ending the session — if so, stand idle until they
    // settle or need attention.
    const toolBlocks = resp.content.filter((b) => b.type === 'tool_use');
    if (toolBlocks.length === 0) {
      if (trackedWorkerPaneIds.size === 0) {
        cb.onComplete();
        return;
      }
      const standby = await standIdleForWorkers(
        executor,
        trackedWorkerPaneIds,
        previousStatus,
        signal,
        cb.onStandby,
        cb.onAutoApprove,
      );
      if (standby.reason === 'aborted') return;
      if (standby.reason === 'all_settled') {
        cb.onComplete();
        return;
      }
      if (standby.reason === 'changed') {
        const note = standby.notes.join('\n');
        const approvedNote =
          standby.autoApproved.length > 0
            ? `\n\n[The harness auto-approved routine permission prompts on: ${standby.autoApproved.join(', ')}]`
            : '';
        messages.push({
          role: 'user',
          content:
            `[Worker pane status update]\n${note}${approvedNote}\n\n` +
            `React to this update: call read_terminal_buffer to see what the worker is showing, then press_key or write_terminal_input to respond, complete_task if the work is done, or briefly summarize and end if the work is complete.`,
        });
        continue;
      }
      // timeout — give the LLM one final turn to wrap up.
      const last = standby.lastStatuses.map((s) => `${s.id}=${s.status}`).join(', ');
      messages.push({
        role: 'user',
        content:
          `[Standby timed out after waiting for worker panes. Last statuses: ${last}.\n\n` +
          `Wrap up now: call read_terminal_buffer for final evidence, complete_task, or tell the user the worker is still running and they should check the pane manually.]`,
      });
      continue;
    }

    // Execute each tool call sequentially (could parallelize later).
    const toolResults: Array<{ type: 'tool_result'; tool_use_id: string; content: string; is_error?: boolean }> = [];
    for (const block of toolBlocks) {
      if (block.type !== 'tool_use') continue;
      if (signal.aborted) return;
      try {
        const result = await executeMcpTool(
          executor,
          block.name,
          block.input as Record<string, unknown>,
          (entry) => {
            cb.onToolCall(block.name, block.input, entry.result_preview, entry.error);
          },
        );
        trackFromToolCall(block.name, block.input as Record<string, unknown>, result);
        toolResults.push({ type: 'tool_result', tool_use_id: block.id, content: result });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        toolResults.push({ type: 'tool_result', tool_use_id: block.id, content: msg, is_error: true });
        cb.onToolCall(block.name, block.input, undefined, msg);
      }
    }

    messages.push({ role: 'user', content: toolResults });
  }

  cb.onComplete();
}
