// Typed wrappers for the "worktrees that ship" commands (`commands/worktrees.rs`): the folder step
// of the "+" menu, the Copies presets of a project, Create PR and the GitHub checks that come back.
import { invoke } from '@tauri-apps/api/core';

export type GitState = 'repo' | 'no_commits' | 'folder' | 'missing';

export interface RecentProject {
  path: string;
  name: string;
  state: GitState;
  last_used_ms: number;
}

export interface GitInitPlan {
  path: string;
  name: string;
  state: 'folder' | 'no_commits';
  /** The commands, as they would be typed. */
  commands: string[];
  files: number;
  too_big: boolean;
  /** Lines of a .gitignore that will be written (empty when the folder has one). */
  gitignore: string[];
}

export interface GitInitOutcome {
  commit: string;
  files: number;
  wrote_gitignore: boolean;
}

export interface ConductorView {
  setup: string[];
  run: string;
  unsupported: string | null;
}

export interface PresetView {
  setup: string[];
  copy: string[];
  run: string;
  conductor: ConductorView | null;
  problem: string | null;
}

export interface SetupStep {
  command: string;
  ok: boolean;
  exit_code: number | null;
  timed_out: boolean;
  tail?: string;
  duration_ms: number;
}

export interface SetupReport {
  status: 'ok' | 'failed';
  copied: string[];
  not_copied: { path: string; reason: string }[];
  steps: SetupStep[];
  log?: string;
  duration_ms: number;
}

export interface CheckItem {
  name: string;
  state: 'pass' | 'fail' | 'pending' | 'skipped' | 'cancelled';
  link?: string;
  workflow?: string;
}

export interface ChecksReport {
  state: 'none' | 'pending' | 'passing' | 'failing';
  items: CheckItem[];
  checked_at_ms: number;
}

export interface PrRef {
  number: number;
  url: string;
  draft: boolean;
  base: string;
  created_at_ms: number;
  checks?: ChecksReport;
}

export interface PrPrepared {
  gh: 'ready' | 'missing' | 'signed_out';
  has_remote: boolean;
  on_github: boolean;
  default_branch: string | null;
  existing: PrRef | null;
  title: string;
  body: string;
  base: string;
  branch: string;
  agent: string | null;
  has_uncommitted: boolean;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export const shipApi = {
  recent: (limit = 8) => call<RecentProject[]>('project_recent', { limit }),
  /** The system folder picker; `null` when cancelled. */
  pickFolder: () => call<RecentProject | null>('project_pick_folder'),
  forget: (path: string) => call<void>('project_forget', { path }),
  /** `repo`, `no_commits`, `folder` (no Git) or `missing`. */
  gitState: (path: string) => call<GitState>('project_git_state', { path }),
  gitInitPlan: (path: string) => call<GitInitPlan>('project_git_init_plan', { path }),
  gitInit: (path: string) => call<GitInitOutcome>('project_git_init', { path }),
  presetGet: (projectPath: string) => call<PresetView>('project_preset_get', { projectPath }),
  presetSet: (projectPath: string, setup: string[], copy: string[], run: string) =>
    call<PresetView>('project_preset_set', { projectPath, setup, copy, run }),
  presetImportConductor: (projectPath: string) =>
    call<PresetView>('project_preset_import_conductor', { projectPath }),
  prPrepare: (projectPath: string | null, handle: string) =>
    call<PrPrepared>('worktree_pr_prepare', { projectPath, handle }),
  prCreate: (projectPath: string | null, handle: string, title: string, body: string, base: string) =>
    call<PrRef>('worktree_pr_create', { projectPath, handle, title, body, base }),
  prChecks: (projectPath: string | null, handle: string, waitSecs = 0) =>
    call<PrRef>('worktree_pr_checks', { projectPath, handle, waitSecs }),
  openUrl: (url: string) => (isTauri() ? invoke<void>('open_external_url', { url }) : Promise.resolve()),
};

export type ShipApi = typeof shipApi;
