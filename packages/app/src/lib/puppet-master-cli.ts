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
    `Act as the active orchestration agent for this request using ${label}.`,
    'Work in the current project. Be concise, report what you are doing, and ask only if blocked.',
    'If you need other agents, describe the split clearly so Puppet Master can route follow-up work.',
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
