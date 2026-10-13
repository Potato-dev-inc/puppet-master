// Toolbelt: MCP servers configured once, written into every agent's own config.
// Wrappers over the `toolbelt_*` Tauri commands (src-tauri/src/commands/toolbelt.rs).
import { invoke } from '@tauri-apps/api/core';

export type ToolTransport = 'stdio' | 'http';
export type ToolScope = 'global' | 'project';

export interface ToolServer {
  /** Empty when adding; the backend makes one. */
  id: string;
  name: string;
  transport: ToolTransport;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
  enabled: boolean;
  scope: ToolScope;
}

/** Shown instead of a secret value; sending it back keeps the stored value. */
export const SECRET_MASK = '********';

export interface ToolbeltAgent {
  id: string;
  label: string;
  installed: boolean;
  supports_url: boolean;
}

export interface ToolbeltUnsupported {
  id: string;
  label: string;
  reason: string;
}

export interface ToolbeltListing {
  servers: ToolServer[];
  agents: ToolbeltAgent[];
  unsupported: ToolbeltUnsupported[];
}

export type ChangeKind = 'add' | 'update' | 'remove' | 'unchanged';

export interface SyncChange {
  key: string;
  name: string;
  kind: ChangeKind;
}

export interface SyncHost {
  agents: { id: string; label: string }[];
  scope: ToolScope;
  path: string;
  format: string;
  status: 'ready' | 'unchanged' | 'skipped' | 'error';
  reason: string | null;
  changes: SyncChange[];
  notes: string[];
  diff: string;
  written: boolean;
  backup: string | null;
}

export interface SyncReport {
  dry_run: boolean;
  hosts: SyncHost[];
  unsupported: ToolbeltUnsupported[];
}

export interface ImportCandidate {
  reference: string;
  agents: { id: string; label: string }[];
  server: ToolServer;
  already: boolean;
}

export interface SyncArgs {
  project?: string | null;
  includePm?: boolean;
}

export interface ToolbeltApi {
  list: () => Promise<ToolbeltListing>;
  add: (server: ToolServer) => Promise<ToolServer>;
  update: (id: string, server: ToolServer) => Promise<ToolServer>;
  remove: (id: string) => Promise<void>;
  syncPreview: (args: SyncArgs) => Promise<SyncReport>;
  syncApply: (args: SyncArgs) => Promise<SyncReport>;
  importPreview: (project?: string | null) => Promise<ImportCandidate[]>;
  importApply: (references: string[], project?: string | null) => Promise<ToolServer[]>;
}

export const toolbeltApi: ToolbeltApi = {
  list: () => invoke<ToolbeltListing>('toolbelt_list'),
  add: async (server) => (await invoke<{ server: ToolServer }>('toolbelt_add', { server })).server,
  update: async (id, server) => (await invoke<{ server: ToolServer }>('toolbelt_update', { id, server })).server,
  remove: async (id) => {
    await invoke('toolbelt_remove', { id });
  },
  syncPreview: ({ project, includePm }) =>
    invoke<SyncReport>('toolbelt_sync_preview', { project: project ?? null, includePm: includePm ?? false }),
  syncApply: ({ project, includePm }) =>
    invoke<SyncReport>('toolbelt_sync_apply', { project: project ?? null, includePm: includePm ?? false }),
  importPreview: async (project) =>
    (await invoke<{ candidates: ImportCandidate[] }>('toolbelt_import_preview', { project: project ?? null })).candidates,
  importApply: async (references, project) =>
    (await invoke<{ added: ToolServer[] }>('toolbelt_import_apply', { project: project ?? null, references })).added,
};

export function errText(err: unknown): string {
  return err instanceof Error ? err.message : typeof err === 'string' ? err : JSON.stringify(err);
}

/** Splits a command line on spaces, keeping "quoted parts" together. */
export function splitArgs(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

export function joinArgs(args: string[] | undefined): string {
  return (args ?? []).map((a) => (/\s/.test(a) ? `"${a}"` : a)).join(' ');
}

/** `KEY=value` lines to a map (blank lines and lines without `=` are ignored). */
export function parsePairs(text: string, sep: '=' | ':' = '='): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const i = line.indexOf(sep);
    if (i <= 0) continue;
    const key = line.slice(0, i).trim();
    if (key) out[key] = line.slice(i + 1).trim();
  }
  return out;
}

export function pairsText(map: Record<string, string> | undefined, sep: '=' | ':' = '='): string {
  return Object.entries(map ?? {}).map(([k, v]) => `${k}${sep === ':' ? ': ' : '='}${v}`).join('\n');
}

export function changeCounts(host: SyncHost): { add: number; update: number; remove: number } {
  const n = { add: 0, update: 0, remove: 0 };
  for (const c of host.changes) if (c.kind !== 'unchanged') n[c.kind] += 1;
  return n;
}
