// Agent doctor client: the `agents_health` / `agents_health_test` Tauri commands (same bodies as
// `GET /agents/health` and `POST /agents/health/test`).
import { invoke } from '@tauri-apps/api/core';

export type HealthState = 'unchecked' | 'ready' | 'not_installed' | 'needs_login' | 'error';

export interface AgentHealth {
  id: string;
  label: string;
  state: HealthState;
  installed: boolean;
  version: string | null;
  /** Plain-language sentence for the state. */
  reason: string;
  install_hint: string | null;
  login_hint: string | null;
  /** A deep check is possible for this agent. */
  can_test: boolean;
}

export interface AgentHealthApi {
  check: () => Promise<AgentHealth[]>;
  test: (agentId: string) => Promise<AgentHealth>;
}

export const agentHealthApi: AgentHealthApi = {
  check: async () => {
    const body = await invoke<{ agents: AgentHealth[] }>('agents_health');
    return body.agents;
  },
  test: (agentId) => invoke<AgentHealth>('agents_health_test', { agentId }),
};

const SPAWNED_KEY = 'pm.cockpit.everSpawned';

/** True once any pane has ever been seen in this install (the first-run hint is hidden after that). */
export function hasEverSpawned(): boolean {
  try {
    return window.localStorage.getItem(SPAWNED_KEY) === '1';
  } catch {
    return true; // storage blocked: never nag
  }
}

export function markSpawned(): void {
  try {
    window.localStorage.setItem(SPAWNED_KEY, '1');
  } catch {
    /* ignore */
  }
}
