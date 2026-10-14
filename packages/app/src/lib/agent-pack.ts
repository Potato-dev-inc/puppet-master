// "Add Puppet Master to your agents": skills + Puppet Master's own MCP entry, written into each agent.
// Wrappers over the `agent_pack_*` Tauri commands (src-tauri/src/commands/agent_pack.rs).
import { invoke } from '@tauri-apps/api/core';

export type FileKind = 'add' | 'update' | 'replace' | 'unchanged';
export type StepStatus = 'ready' | 'unchanged' | 'skipped' | 'error';

export interface SkillsStep {
  status: StepStatus;
  path: string;
  shared_with: string[];
  files: { name: string; kind: FileKind }[];
  reason: string | null;
  written: boolean;
}

export interface McpStep {
  status: StepStatus;
  path: string;
  format: string;
  shared_with: string[];
  change: FileKind;
  reason: string | null;
  diff: string;
  written: boolean;
  backup: string | null;
}

export interface PackAgent {
  id: string;
  label: string;
  installed: boolean;
  skills: SkillsStep | null;
  mcp: McpStep | null;
}

export interface PackReport {
  dry_run: boolean;
  agents: PackAgent[];
  unsupported: { id: string; label: string; installed: boolean }[];
  /** Files and config entries an Undo would put back. */
  recorded: number;
}

export interface UndoReport {
  restored: string[];
  removed: string[];
  skipped: { path: string; reason: string }[];
  recorded: number;
}

export interface AgentPackApi {
  preview: () => Promise<PackReport>;
  apply: () => Promise<PackReport>;
  undo: () => Promise<UndoReport>;
}

export const agentPackApi: AgentPackApi = {
  preview: () => invoke<PackReport>('agent_pack_preview'),
  apply: () => invoke<PackReport>('agent_pack_apply'),
  undo: () => invoke<UndoReport>('agent_pack_undo'),
};

/** One row per set of agents that share the same folders: OpenCode and OpenCode (API) read one file. */
export interface PackRow {
  key: string;
  labels: string[];
  installed: boolean;
  skills: SkillsStep | null;
  mcp: McpStep | null;
}

export function groupAgents(agents: PackAgent[]): PackRow[] {
  const rows: PackRow[] = [];
  for (const a of agents) {
    const key = `${a.skills?.path ?? ''}|${a.mcp?.path ?? ''}`;
    const row = rows.find((r) => r.key === key);
    if (row) {
      row.labels.push(a.label);
      row.installed ||= a.installed;
    } else {
      rows.push({ key, labels: [a.label], installed: a.installed, skills: a.skills, mcp: a.mcp });
    }
  }
  return rows;
}

/** Rows worth showing: installed ones, or ones with something to do. */
export function visibleRows(rows: PackRow[]): PackRow[] {
  return rows.filter((r) => r.installed || r.skills?.status === 'ready' || r.mcp?.status === 'ready');
}

export function pendingCount(rows: PackRow[]): number {
  return rows.filter((r) => r.skills?.status === 'ready' || r.mcp?.status === 'ready').length;
}
