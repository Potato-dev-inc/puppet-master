import type { BridgeClient } from './bridge';
import type { PuppetMasterCallbacks } from './puppet-master';
import {
  BACKEND_AGENT,
  BACKEND_LABEL,
  type CliOrchestratorBackend,
  ensureOrchestratorPane,
} from './orchestrator-panes';
import { tauri } from './tauri';

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function buildDelegationPrompt(label: string, userPrompt: string): string {
  return [
    'You are being driven by Puppet Master from the sidebar.',
    `Act as the manager/orchestration agent for this request using ${label}.`,
    'You are the orchestrator, not a coder. Your primary task is to distribute work to worker agents, coordinate progress, unblock their TUIs, and report verified results.',
    'ABSOLUTE MANAGER BOUNDARY: do not implement code yourself, even if the user asks you to fix something quickly.',
    'Do not use your native coding tools for implementation work: no Bash/shell commands for project work, no Edit, no Write, no MultiEdit, no file creation, no direct source-file modification, no direct test/build/lint execution.',
    'For coding, shell, test, repo-inspection, or bug-fix work, use Puppet Master MCP tools to create/claim a task, acquire locks, build a context pack, spawn or reuse a worker pane, and write the assignment to that worker pane.',
    'If you discover a bug while reviewing buffers or evidence, assign a worker to patch it and another worker or shell worker to verify it. Do not patch it yourself.',
    'Your allowed direct work is coordination only: list/read panes, spawn/reuse workers, send prompts/keys to workers, manage tasks/locks, read worker buffers, and summarize evidence.',
    'Be concise, report which worker pane owns the task, and ask only if blocked.',
    '',
    'User request:',
    userPrompt,
  ].join('\n');
}

/**
 * CLI-backed orchestration: spawn (or reuse) a dedicated agent pane with MCP
 * configured, send the user prompt, poll scrollback for the reply.
 */
export async function runPuppetMasterCliLoop(
  backend: CliOrchestratorBackend,
  _bridge: BridgeClient | null,
  userPrompt: string,
  cb: PuppetMasterCallbacks,
  signal: AbortSignal,
): Promise<void> {
  try {
    const label = BACKEND_LABEL[backend];
    const paneId = await ensureOrchestratorPane(backend, await tauri.getProjectPath());
    if (signal.aborted) return;

    cb.onToolCall('spawn_agent', { agent_type: BACKEND_AGENT[backend], pane_id: paneId }, `using pane ${paneId}`);
    cb.onAssistantText(`Delegated to ${label} in pane ${paneId}.\n\n`);

    await tauri.writeInput(paneId, buildDelegationPrompt(label, userPrompt), true);
    cb.onToolCall('write_terminal_input', { pane_id: paneId }, 'prompt submitted');

    await sleep(2500);
    if (signal.aborted) return;

    const buffer = await tauri.readBuffer(paneId, 120);
    cb.onToolCall('read_terminal_buffer', { pane_id: paneId, lines: 120 }, buffer.slice(0, 200));
    cb.onAssistantText(
      buffer.trim()
        ? `Recent ${label} output:\n\n${buffer.trim()}`
        : `${label} accepted the prompt; no readable output yet.`,
    );
    cb.onComplete();
  } catch (err) {
    cb.onError(err instanceof Error ? err : new Error(String(err)));
  }
}
