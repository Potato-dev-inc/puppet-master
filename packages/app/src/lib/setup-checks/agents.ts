// Agents: one row per installed agent, from the same health check the Agent Doctor uses (`agents_health`).
// The sentence is the health `reason`, which the backend already wrote in plain words.
import type { AgentHealth } from '../agent-health';
import type { CheckRow } from './types';

export function checkAgents(agents: AgentHealth[]): CheckRow[] {
  return agents
    .filter((a) => a.installed)
    .map((a): CheckRow => {
      if (a.state === 'ready') return { id: `agent-${a.id}`, status: 'ok', sentence: `${a.label} is signed in`, fix: null };
      if (a.state === 'needs_login') {
        return {
          id: `agent-${a.id}`,
          status: 'warn',
          sentence: `${a.label} needs sign-in`,
          fix: { kind: 'signin', label: 'Sign in', agentId: a.id },
        };
      }
      if (a.state === 'error') return { id: `agent-${a.id}`, status: 'fail', sentence: a.reason || `${a.label} has a problem`, fix: null };
      return { id: `agent-${a.id}`, status: 'warn', sentence: `${a.label} is not tested yet`, fix: null };
    });
}
