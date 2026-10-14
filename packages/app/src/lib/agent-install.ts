// "Set up agents" Install client: the `agents_install_*` Tauri commands (src-tauri/src/agent_install).
// Tauri commands only: there is no bridge route, so an agent can never start an install.
import { invoke } from '@tauri-apps/api/core';
import type { AgentHealth } from './agent-health';

export interface InstallStep {
  index: number;
  /** What this step installs, e.g. "Node.js" or the agent's name. */
  label: string;
  /** `git` / `node` for a prerequisite, null for the agent itself. */
  tool: string | null;
  method: 'winget' | 'npm' | 'script';
  /** The exact text that will run. It is also what Run sends back, so the app can refuse a changed command. */
  command: string;
  /** The vendor page this comes from. */
  source: string;
  source_host: string;
  /** A script downloaded from the internet: needs a second confirmation. */
  high_risk: boolean;
  note: string;
}

export interface InstallPlan {
  agent_id: string;
  label: string;
  available: boolean;
  /** Plain reason when `available` is false. */
  reason: string | null;
  /** The old one-line hint, shown with Copy when nothing can be run from here. */
  hint: string | null;
  steps: InstallStep[];
}

export interface InstallStarted {
  pane_id: string;
  command: string;
  label: string;
}

export interface InstallStatus {
  known: boolean;
  done: boolean;
  exit_code: number | null;
  message: string | null;
  closed?: boolean;
}

export interface InstallFinish {
  path_added: string[];
  health: AgentHealth;
  /** Still not found after refreshing PATH: a restart gives every process the new PATH. */
  needs_restart: boolean;
}

export interface AgentInstallApi {
  plan: (agentId: string) => Promise<InstallPlan>;
  run: (agentId: string, step: number, shownCommand: string, acknowledgedRisk: boolean) => Promise<InstallStarted>;
  status: (paneId: string) => Promise<InstallStatus>;
  finish: (agentId: string) => Promise<InstallFinish>;
}

export const agentInstallApi: AgentInstallApi = {
  plan: (agentId) => invoke<InstallPlan>('agents_install_plan', { agentId }),
  run: (agentId, step, shownCommand, acknowledgedRisk) =>
    invoke<InstallStarted>('agents_install_run', { agentId, step, shownCommand, acknowledgedRisk }),
  status: (paneId) => invoke<InstallStatus>('agents_install_status', { paneId }),
  finish: (agentId) => invoke<InstallFinish>('agents_install_finish', { agentId }),
};

/** "code.claude.com" style host for the "this comes from" line. */
export function sourceLine(step: InstallStep): string {
  return `This comes from ${step.source_host}.`;
}
