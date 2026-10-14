// Typed wrappers for the agent-authority commands (`commands/agent_authority.rs`): the two switches
// and the read-only audit list. Only the desktop window can call them; the phone has no route.
import { invoke } from '@tauri-apps/api/core';

export interface AgentAuthority {
  /** A: ask before an agent controls a pane it did not start. */
  ask_before_agent_control: boolean;
  /** B: agents may merge a separate copy. */
  agents_may_merge: boolean;
}

export type AuditKind = 'approval' | 'take_over' | 'consent' | 'pairing' | 'shell_exec' | 'gate_dismissal' | 'merge' | 'setting';

export interface AuditEntry {
  seq: number;
  ts_ms: number;
  kind: AuditKind;
  /** `desktop`, `device`, `agent` or `local`. */
  who: 'desktop' | 'device' | 'agent' | 'local';
  /** One plain sentence. */
  summary: string;
}

export interface AuditVerify {
  ok: boolean;
  entries: number;
  broken: { file: string; line: number; reason: string } | null;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const agentAuthorityApi = {
  get: () => call<AgentAuthority>('agent_authority_get'),
  set: (patch: Partial<AgentAuthority>) => call<AgentAuthority>('agent_authority_set', { askBeforeAgentControl: patch.ask_before_agent_control, agentsMayMerge: patch.agents_may_merge }),
  auditRecent: (limit = 50) => call<AuditEntry[]>('audit_recent', { limit }),
  auditVerify: () => call<AuditVerify>('audit_verify'),
};

export type AgentAuthorityApi = typeof agentAuthorityApi;

/** Who did it, in words ("You", "A paired phone", "An agent"). */
export function whoText(who: AuditEntry['who']): string {
  switch (who) {
    case 'desktop':
      return 'You';
    case 'device':
      return 'A paired phone';
    case 'agent':
      return 'An agent';
    default:
      return 'A program on this computer';
  }
}
