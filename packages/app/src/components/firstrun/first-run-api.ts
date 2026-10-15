// The Tauri commands of the first-run welcome (`commands/first_run.rs`), typed. The UI never says where
// the first task runs or whether it may write: Rust always makes a separate copy, read-only when the
// agent can be locked to it. The folder check and picker are Simple mode's (`plain_check_folder`).
import { invoke } from '@tauri-apps/api/core';
import type { FirstRunState } from './model';

export interface TaskStarted {
  project: string;
  handle: string;
  /** Set when the run is a visible pane (an agent that cannot be locked to read-only). */
  pane_id: string | null;
  /** True when read-only is enforced by the agent. */
  enforced: boolean;
}

export interface TaskStatus {
  status: string;
  result: string | null;
  error: { code: string; message?: string } | null;
  receipt: unknown | null;
  pane_id: string | null;
}

export interface FirstRunApi {
  state: () => Promise<FirstRunState>;
  startTask: (project: string, agent: string) => Promise<TaskStarted>;
  taskStatus: (project: string, handle: string) => Promise<TaskStatus>;
}

export const firstRunApi: FirstRunApi = {
  state: () => invoke<FirstRunState>('first_run_state'),
  startTask: (project, agent) => invoke<TaskStarted>('first_run_start_task', { project, agent }),
  taskStatus: (project, handle) => invoke<TaskStatus>('first_run_task_status', { project, handle }),
};

export const isFinalStatus = (status: string): boolean => status === 'completed' || status === 'failed' || status === 'cancelled';

/** Mirrors `agent_runs::pane_api`: the code at the front of "CODE: message", or null. */
export function codeOf(err: unknown): string | null {
  const text = err instanceof Error ? err.message : String(err);
  const match = /^([A-Z][A-Z_]+):/.exec(text);
  return match ? match[1] : null;
}
