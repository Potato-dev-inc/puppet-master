// Declarative agent definitions for Settings > Agents (SPEC 7.5): typed wrappers over the Tauri
// commands in commands/agents.rs. The webview is the "app session" the mutating routes require.
import { invoke } from '@tauri-apps/api/core';

export type DefinitionState = 'active' | 'unapproved' | 'invalid' | 'shadowed' | 'pending_consent' | string;

export interface DefinitionError {
  code: string;
  message: string;
  path: string | null;
  line: number | null;
  column: number | null;
}

export interface DefinitionRow {
  id: string | null;
  file: string;
  source: string;
  state: DefinitionState;
  errors: DefinitionError[];
  fingerprint: string | null;
  shadowed_by: string | null;
}

export interface SourceRow {
  name: string;
  enabled: boolean;
  path: string | null;
  /** null for sources that need no consent (bundled, user). */
  consent: boolean | null;
}

export interface DefinitionsListing {
  sources: SourceRow[];
  definitions: DefinitionRow[];
  diff?: { added: string[]; removed: string[]; changed: string[] };
}

export interface DefinitionPreview {
  ok: boolean;
  errors: DefinitionError[];
  rendered: { headless_argv: string[] | null; tui_argv: string[] | null };
}

/** The backend the tab talks to; tests pass a mock. */
export interface FixtureResult {
  fixture: string;
  passed: boolean;
  failures: string[];
}

export interface AgentDefinitionsApi {
  /** Runs the definition file against a folder of captures with *.expect.toml files. */
  checkFile(path: string, fixturesDir?: string): Promise<FixtureResult[]>;
  list(): Promise<DefinitionsListing>;
  reload(): Promise<DefinitionsListing>;
  preview(file: string): Promise<DefinitionPreview>;
  approve(file: string, fingerprint: string): Promise<unknown>;
  revoke(id: string): Promise<unknown>;
  consent(kind: string, path: string, enabled: boolean): Promise<unknown>;
}

export const tauriAgentDefinitionsApi: AgentDefinitionsApi = {
  checkFile: (path, fixturesDir) => invoke<FixtureResult[]>('agents_check_file', { path, fixturesDir: fixturesDir || null }),
  list: () => invoke<DefinitionsListing>('agents_definitions_list', undefined),
  reload: () => invoke<DefinitionsListing>('agents_definitions_reload', undefined),
  preview: (file) => invoke<DefinitionPreview>('agents_definition_preview', { file }),
  approve: (file, fingerprint) => invoke('agents_definition_approve', { file, fingerprint }),
  revoke: (id) => invoke('agents_definition_revoke', { id }),
  consent: (kind, path, enabled) => invoke('agents_definitions_consent', { kind, path, enabled }),
};

const STATE_LABELS: Record<string, string> = {
  active: 'Active',
  unapproved: 'Needs approval',
  invalid: 'Invalid',
  shadowed: 'Shadowed',
  pending_consent: 'Awaiting consent',
};

export function stateLabel(state: DefinitionState): string {
  return STATE_LABELS[state] ?? state;
}

const SOURCE_LABELS: Record<string, string> = {
  bundled: 'Bundled',
  user: 'User',
  project: 'Project',
  extra: 'Extra',
};

export function sourceLabel(source: string): string {
  return SOURCE_LABELS[source] ?? source;
}

/** Shell-ish display of an argv; display only, never executed. */
export function formatArgv(argv: string[]): string {
  return argv.map((arg) => (/^[\w./:=@%+,-]+$/.test(arg) ? arg : JSON.stringify(arg))).join(' ');
}

export function fileName(path: string): string {
  return path.split(/[\/]/).pop() ?? path;
}

/** Warnings shown before approving (APR-3). */
export function approvalWarnings(row: DefinitionRow, preview: DefinitionPreview | null): string[] {
  const warnings = ['Approving lets Puppet Master run this program with your permissions.'];
  if (row.state === 'active') warnings.push('This definition is already approved.');
  if (preview && !preview.ok) warnings.push('The definition has errors and cannot be approved.');
  if (row.shadowed_by) warnings.push(`Shadowed by ${fileName(row.shadowed_by)}; approving will not make it the active one.`);
  return warnings;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
