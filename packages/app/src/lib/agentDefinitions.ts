// Declarative agent definitions for Settings > Agents (SPEC 7.5): typed wrappers over the Tauri
// commands in commands/agents.rs. The webview is the "app session" the mutating routes require.
import { invoke } from '@tauri-apps/api/core';

export type DefinitionState =
  | 'active'
  | 'needs_approval'
  | 'approval_stale'
  | 'unapproved'
  | 'invalid'
  | 'shadowed'
  | 'pending_consent'
  | string;

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
  /** Approved, but the program now resolves elsewhere than the path pinned at approval. */
  binary_changed?: boolean;
}

export interface ProjectConsentRow {
  path: string;
  files: string[];
}

export interface ExtraDirRow {
  path: string;
  consent: boolean;
  exists: boolean;
}

/** What the consent banner shows for the open project (names only; the files are not read). */
export interface ProjectScan {
  project: string;
  files: string[];
  consented: boolean;
  enabled: boolean;
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
  /** Consented projects (canonical paths) and the definition files each one holds. */
  projects?: ProjectConsentRow[];
  extra_dirs?: ExtraDirRow[];
  diff?: { added: string[]; removed: string[]; changed: string[] };
}

/** The argv, pinned binary and time of the approval on record for a file (absent: never approved). */
export interface ApprovedSnapshot {
  headless_argv: string[] | null;
  tui_argv: string[] | null;
  binary: string;
  approved_at: string;
  fingerprint: string;
}

export interface DefinitionPreview {
  ok: boolean;
  errors: DefinitionError[];
  rendered: { headless_argv: string[] | null; tui_argv: string[] | null };
  /** Backend-computed flags: --auto / --dangerously / --yolo / --force, env keys, interpreters. */
  warnings?: string[];
  approved?: ApprovedSnapshot | null;
  binary_changed?: boolean;
}

/** The backend the tab talks to; tests pass a mock. */
export interface FixtureResult {
  fixture: string;
  passed: boolean;
  failures: string[];
}

export interface DraftFile {
  name: string;
  path: string;
  modified_ms: number;
  size: number;
}

export interface DraftsListing {
  dir: string;
  drafts: DraftFile[];
}

export interface RecorderStopResult {
  path: string;
  id: string;
  notes: string[];
  snapshots: number;
}

export interface AgentDefinitionsApi {
  /** Draft definitions written by the session recorder (never loaded, never approved). */
  draftsList?(): Promise<DraftsListing>;
  /** Validates a draft through the same checks as /agents/definitions/validate. */
  draftValidate?(path: string): Promise<DefinitionPreview>;
  /** Writes an UNAPPROVED experimental generic definition for a bare command name, then rescans. */
  addGeneric(command: string): Promise<DefinitionsListing>;
  /** Sharing (P5-6): asks for a .toml target, writes the definition (+ fixtures zip). null = cancelled. */
  exportDefinition(id: string): Promise<{ path: string; fixtures_zip: string | null } | null>;
  /** Sharing (P5-6): asks for a .toml file, places it in the user dir UNAPPROVED. null = cancelled. */
  importDefinition(): Promise<DefinitionsListing | null>;
  /** Runs the definition file against a folder of captures with *.expect.toml files. */
  checkFile(path: string, fixturesDir?: string): Promise<FixtureResult[]>;
  list(): Promise<DefinitionsListing>;
  reload(): Promise<DefinitionsListing>;
  preview(file: string): Promise<DefinitionPreview>;
  approve(file: string, fingerprint: string): Promise<unknown>;
  revoke(id: string): Promise<unknown>;
  consent(kind: string, path: string, enabled: boolean): Promise<unknown>;
  /** SRC-5: switch the user / project / extra source on or off. */
  sourceSet(name: string, enabled: boolean): Promise<unknown>;
  /** SRC-4: add or remove an extra definition directory (a new one has no consent). */
  extraDir(action: 'add' | 'remove', path: string): Promise<unknown>;
  /** The project the app has open, if any. */
  projectPath(): Promise<string>;
  /** Consent banner data for a project: the definition file names found there. */
  projectScan(project: string): Promise<ProjectScan>;
}

export const tauriAgentDefinitionsApi: AgentDefinitionsApi = {
  addGeneric: (command) => invoke<DefinitionsListing>('agents_add_generic', { command }),
  exportDefinition: async (id) => {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const target = await save({ defaultPath: `${id}.toml`, filters: [{ name: 'Agent definition', extensions: ['toml'] }] });
    if (!target) return null;
    return invoke<{ path: string; fixtures_zip: string | null }>('agents_export_definition', { id, target });
  },
  importDefinition: async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const source = await open({ multiple: false, filters: [{ name: 'Agent definition', extensions: ['toml'] }] });
    if (!source || Array.isArray(source)) return null;
    return invoke<DefinitionsListing>('agents_import_definition', { source });
  },
  checkFile: (path, fixturesDir) => invoke<FixtureResult[]>('agents_check_file', { path, fixturesDir: fixturesDir || null }),
  list: () => invoke<DefinitionsListing>('agents_definitions_list', undefined),
  reload: () => invoke<DefinitionsListing>('agents_definitions_reload', undefined),
  preview: (file) => invoke<DefinitionPreview>('agents_definition_preview', { file }),
  approve: (file, fingerprint) => invoke('agents_definition_approve', { file, fingerprint }),
  revoke: (id) => invoke('agents_definition_revoke', { id }),
  consent: (kind, path, enabled) => invoke('agents_definitions_consent', { kind, path, enabled }),
  sourceSet: (name, enabled) => invoke('agents_source_set', { name, enabled }),
  extraDir: (action, path) => invoke('agents_extra_dir', { action, path }),
  projectPath: () => invoke<string>('get_project_path_cmd', undefined),
  projectScan: (project) => invoke<ProjectScan>('agents_project_scan', { project }),
  draftsList: () => invoke<DraftsListing>('agents_drafts_list', undefined),
  draftValidate: (path) => invoke<DefinitionPreview>('agents_draft_validate', { path }),
};

/** Pane toolbar: record a session, then stop to get a draft file path (REC-1, REC-4). */
export interface RecorderApi {
  start(paneId: string): Promise<{ recording: boolean; snapshots: number }>;
  status(paneId: string): Promise<{ recording: boolean; snapshots: number }>;
  stop(paneId: string): Promise<RecorderStopResult>;
}

export const tauriRecorderApi: RecorderApi = {
  start: (paneId) => invoke('recorder_start', { paneId }),
  status: (paneId) => invoke('recorder_status', { paneId }),
  stop: (paneId) => invoke('recorder_stop', { paneId }),
};

const STATE_LABELS: Record<string, string> = {
  active: 'Active',
  needs_approval: 'Needs approval',
  approval_stale: 'Changed since approval',
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

/** States in which the user can review and approve the file (a changed binary needs it too). */
export function canReview(row: DefinitionRow): boolean {
  if (!row.fingerprint) return false;
  return ['needs_approval', 'approval_stale', 'unapproved'].includes(row.state) || (row.state === 'active' && !!row.binary_changed);
}

export interface DiffLine {
  kind: 'same' | 'added' | 'removed';
  text: string;
}

/** Argument-level diff (longest common subsequence) of the approved argv against the current one. */
export function argvDiff(approved: string[], current: string[]): DiffLine[] {
  const m = approved.length;
  const n = current.length;
  const lcs: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      lcs[i][j] = approved[i] === current[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < m && j < n) {
    if (approved[i] === current[j]) {
      out.push({ kind: 'same', text: approved[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: 'removed', text: approved[i++] });
    } else {
      out.push({ kind: 'added', text: current[j++] });
    }
  }
  while (i < m) out.push({ kind: 'removed', text: approved[i++] });
  while (j < n) out.push({ kind: 'added', text: current[j++] });
  return out;
}

/** Warnings shown before approving (APR-3, P5-4): the fixed notice plus the backend's flags. */
export function approvalWarnings(row: DefinitionRow, preview: DefinitionPreview | null): string[] {
  const warnings = ['Approving lets Puppet Master run this program with your permissions.'];
  for (const warning of preview?.warnings ?? []) warnings.push(warning);
  if (row.binary_changed || preview?.binary_changed) {
    warnings.push('The program now resolves to a different file than the one you approved. Approve again only if you expect that.');
  }
  if (row.state === 'active' && !row.binary_changed) warnings.push('This definition is already approved.');
  if (preview && !preview.ok) warnings.push('The definition has errors and cannot be approved.');
  if (row.shadowed_by) warnings.push(`Shadowed by ${fileName(row.shadowed_by)}; approving will not make it the active one.`);
  return warnings;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
