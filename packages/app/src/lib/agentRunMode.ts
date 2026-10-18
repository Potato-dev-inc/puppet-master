// "How agent runs work": the per-agent choice between a visible pane and One-task mode. Backed by the
// Tauri commands in `commands/agent_run_mode.rs` (the desktop window only; the phone and agents have no route).
import { invoke } from '@tauri-apps/api/core';

export type RunMode = 'visible' | 'one_task';

/** Why One-task mode is not available (stable codes from `agent_run_mode/verify.rs`). */
export type NotVerifiedReason =
  | 'no_headless_mode'
  | 'not_approved'
  | 'not_tested'
  | 'changed_since_test'
  | 'built_in'
  | string;

export interface RunModeAgent {
  id: string;
  label: string | null;
  headless_supported: boolean;
  headless_verified: boolean;
  headless_verified_reason: NotVerifiedReason | null;
  run_mode: RunMode;
}

export interface RunModeView {
  agents: RunModeAgent[];
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const tauriAgentRunModeApi = {
  get: () => call<RunModeView>('agent_run_mode_get'),
  set: (id: string, mode: RunMode) => call<RunModeView>('agent_run_mode_set', { id, mode }),
};

export type AgentRunModeApi = typeof tauriAgentRunModeApi;

export const VISIBLE_LABEL = 'Visible pane (you can watch it)';
export const ONE_TASK_LABEL = 'One-task mode (verified answers, model and session; no pane)';

/** Plain words for why One-task mode cannot be picked. Empty when it can. */
export function notVerifiedText(reason: NotVerifiedReason | null | undefined): string {
  switch (reason) {
    case null:
    case undefined:
      return '';
    case 'no_headless_mode':
      return 'This agent has no one-task mode.';
    case 'not_approved':
      return 'Approve this agent first.';
    case 'not_tested':
      return 'One-task mode needs the wizard test to pass for this agent first.';
    case 'changed_since_test':
      return 'This agent changed after the wizard tested it. Run the wizard test again.';
    default:
      return 'One-task mode is not verified for this agent.';
  }
}

/** What the person sees today for one agent: a saved One-task choice that no longer holds shows as a note. */
export function lapsedNote(agent: RunModeAgent): string {
  return agent.run_mode === 'one_task' && !agent.headless_verified
    ? 'Set to One-task mode, but it is not verified now, so runs use a visible pane.'
    : '';
}
