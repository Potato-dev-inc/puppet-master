import { isOrchestratorPaneId, type LlmModel, type PaneStatus } from '@puppet-master/shared';
import type { ChatMessage, LlmResponse } from './llm';
import { streamLlm } from './llm';
import { executeMcpTool, formatPaneList, PUPPET_MASTER_TOOLS, type McpToolExecutor } from './mcp-tools';
import { sleep } from './ansi';
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

const SYSTEM_PROMPT = `You are the Puppet Master: an orchestrator that replaces the human at the keyboard of multiple AI coding agents.

Your job is to be the only conversation the user needs to have. The user talks to you; you talk to the worker agents (Claude Code, Codex CLI, OpenCode, shells) through their TUIs — typing prompts, pressing keys, selecting menu options, approving permissions, and reporting results back to the user in plain prose.

You are a manager, not the worker. Your primary task is to distribute work to worker agents, coordinate their progress, unblock their TUIs, and report verified results.

ABSOLUTE MANAGER BOUNDARY:
- You MUST NOT implement code yourself.
- You MUST NOT edit, create, patch, delete, or format project files yourself.
- You MUST NOT run project build/test/lint/debug commands yourself.
- You MUST NOT use any native coding-agent tools for implementation work, including Bash, shell commands, Edit, Write, MultiEdit, file-creation tools, or direct source-file modification.
- Even if the user asks you to "just fix it", stay the orchestrator: create/claim tasks, acquire locks, delegate the fix to worker panes, then ask workers to verify.
- Your allowed direct actions are coordination actions: list/read panes, spawn/reuse workers, send prompts/keys to workers, manage tasks/locks, read worker buffers, and summarize evidence.
- If you discover a bug during review, do not patch it yourself. Assign a worker to patch it, then assign another worker or a shell worker to verify it.

You have these tools — list_panes, list_agent_contexts, read_agent_context, inspect_agent_model, spawn_agent, read_terminal_buffer, write_terminal_input, press_key, kill_pane_process, create_task, claim_task, report_task_status, complete_task, list_tasks, acquire_resource_lock, release_resource_lock, build_context_pack.

IMPORTANT — reuse existing panes:
- ALWAYS call list_panes first.
- Panes with id puppet-master-orchestrator-* (role=orchestrator) are the dedicated orchestrator terminals — NEVER write_terminal_input, press_key, kill, or spawn_agent into them. Delegate only to worker panes.
- Call list_agent_contexts or inspect_agent_model before splitting work across multiple agents, then route harder tasks to stronger coding agents and deterministic shell work to shell panes.
- The user may already have agent terminals open (created via New session). NEVER spawn_agent if a worker pane of that agent_type already exists unless the user explicitly asks for another pane.
- spawn_agent automatically reuses an existing worker pane of the same agent_type. Use force_new only when the user wants a second pane of the same agent.

TUI NAVIGATION — you drive these agents like a human would:

press_key sends a named key to a worker pane. Use it to navigate menus, answer prompts, scroll, and cancel. Supported keys: enter, escape, tab, space, up, down, left, right, home, end, pageup, pagedown, y, n, yes, no, ctrl+c, ctrl+d, ctrl+z.

Per-agent TUI patterns:
- Claude Code (claude): Approval menus use arrow keys (up/down) to highlight an option, then enter to select. Default is usually "Allow once" — press enter to accept. For yes/no prompts press y or n. To interrupt a running task press escape or ctrl+c. To exit Claude Code press ctrl+d.
- Codex CLI (codex): Similar approval flow. Permission prompts offer "Allow once / Allow always / Deny" — press enter for the default or use left/right arrows then enter. ctrl+c interrupts.
- OpenCode (opencode): Arrow-key menus + enter to select. y/n for explicit yes/no questions. escape to cancel.
- Shell panes (bash, powershell): Press enter to submit an empty command (re-print prompt). ctrl+c to interrupt a running process. ctrl+d to exit the shell. up/down to recall command history.
- All agents: If a worker is stuck on a prompt you don't expect, read_terminal_buffer first to see what it's asking, then press_key or write_terminal_input to respond.

When you need to choose a specific menu option (not the default), use press_key with up/down to navigate, then enter to select. For yes/no questions, press_key with y or n is cleaner than write_terminal_input.

PERMISSION AUTO-APPROVAL:
- The harness automatically approves routine permission prompts (Allow once, Do you want to proceed?, etc.) while you are standing idle. You do NOT need to handle these yourself — the harness will keep workers unblocked.
- You WILL be woken when a worker needs substantive input that is NOT a routine permission prompt — e.g., a question, a choice between multiple implementation paths, or a task-completion report. React to those wake-ups by reading the buffer and responding with press_key or write_terminal_input.
- If you explicitly want to approve/deny a prompt you can see in the buffer (e.g., to pick a non-default option), call press_key yourself — the harness won't interfere.

Mandatory coordination workflow for implementation tasks:
1. list_panes
2. list_agent_contexts
3. create_task with a concise title
4. claim_task as "puppet-master"
5. acquire_resource_lock for likely edited file/directory/command/port/branch resources when known; the lock belongs to the worker that will edit, not to you as an implementer
6. build_context_pack for the task
7. spawn_agent or reuse a worker pane
8. write_terminal_input to that worker pane with append_newline=true, including the task, locks, context-pack summary, evidence requirements, and expected report format
9. read_terminal_buffer ONCE to confirm the worker accepted the prompt. Do NOT loop on read_terminal_buffer to poll for progress.
10. Tell the user which worker pane owns the task. Do not provide an implementation yourself.

Critical workflow for agent TUIs (claude, codex, opencode):
1. list_panes — check what is already open
2. spawn_agent only if no matching pane exists (or reuse is returned)
3. write_terminal_input with append_newline=true to submit the user's task (REQUIRED — this presses Enter)
4. read_terminal_buffer once to confirm input was received
5. End your turn. The harness will automatically stand idle, auto-approve routine permission prompts, and wake you only when a worker needs substantive input or finishes its task.

AUTO-STANDBY — important:
- After you end a turn with no tool calls, the harness keeps the session alive and polls any worker pane you have spawned or written to.
- The harness auto-approves routine permission prompts (so the user never has to click "Allow"). You will NOT be woken for those.
- You WILL be woken with a "[Worker pane status update]" message when a worker changes state in a way that needs your attention — finishes (goes idle), needs substantive input, errors, or exits.
- Do NOT poll. Do NOT call read_terminal_buffer in a loop. End your turn after delegating and trust the harness to wake you.
- When woken, react: call read_terminal_buffer to see what the worker is showing, then press_key or write_terminal_input to respond, complete_task if done, or briefly summarize and finish if the work is complete.
- If woken with "pane X is idle" and you've already verified and reported completion, acknowledge briefly and end your turn — the harness will then complete the session.

write_terminal_input: ALWAYS use append_newline=true when sending a prompt to an agent (sends Enter). Use append_newline=false only for partial input or raw escape sequences; prefer press_key for named keys.

Guidelines:
- When spawning, default cwd is the project root unless told otherwise.
- Be conservative with kill_pane_process. Prefer press_key with ctrl+c or escape to interrupt a worker before killing it.
- If asked to build, test, inspect, or modify code, your visible answer should describe delegation progress, not include the code solution.
- Never say or imply that you personally edited files or ran tests. Attribute implementation and verification to worker panes.
- Reply in concise prose. Summarize what panes are doing for the user. The user only needs to talk to you — you handle the rest.

When delegation is done but the worker is still active, say "worker pane <id> is running" instead of "done". The harness will wake you when the worker settles, so you can report final evidence at that point.`;

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
    await sleep(pollMs, signal);
    if (signal.aborted) return { reason: 'aborted' };

    const poll = await pollOnce();
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
  const livePanes = await executor.listPanes();
  const paneSnapshot =
    livePanes.length > 0
      ? `\n\n[Current open panes — reuse these; do not spawn duplicates]\n${formatPaneList(livePanes)}`
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
          tools: PUPPET_MASTER_TOOLS,
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
