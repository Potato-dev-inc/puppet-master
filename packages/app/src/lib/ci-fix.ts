// Typed wrappers for "Red to Green" (`ci_fix/`): Puppet Master watches the CI of a pull request it opened,
// asks the person before an agent reads the failed log, and asks again before anything is pushed.
// Desktop only. Rust structs keep their snake_case names here; the watch and fix views are camelCase.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** Labs > Red to Green settings (Rust `ci_fix::model::Settings`). The backend clamps every number to its cap. */
export interface CiFixSettings {
  enabled: boolean;
  tries_per_pr: number;
  turns_per_try: number;
  minutes_per_try: number;
  fixes_per_day: number;
}

/** The `gh` login state (Rust `GhView`). */
export interface GhView {
  state: 'ready' | 'missing' | 'signed_out';
  /** The GitHub login `gh` is signed in as; empty when not signed in. */
  login: string;
}

/** A watch's state (Rust `WatchState`, snake_case). */
export type WatchStateName = 'watching' | 'paused' | 'stopped' | 'closed' | 'merged';

/** A fix's state (Rust `FixState`, snake_case). */
export type FixStateName =
  | 'asking'
  | 'declined'
  | 'expired'
  | 'queued'
  | 'starting'
  | 'running'
  | 'checking'
  | 'ask_push'
  | 'pushing'
  | 'pushed'
  | 'green'
  | 'still_red'
  | 'no_changes'
  | 'held'
  | 'not_pushed'
  | 'failed';

/** What the log read found (Rust `LogNotes`). Counts only; no log text. */
export interface CiLogNotes {
  lines: number;
  cut_lines: number;
  hidden_secrets: number;
  invisible_chars: number;
  jobs_read: number;
  jobs_without_log: number;
  truncated: boolean;
}

/** One watched pull request, as the Labs list and the PR block show it (`WatchView`, camelCase). */
export interface WatchView {
  id: string;
  repo: string;
  prNumber: number;
  prUrl: string;
  branch: string;
  agentLabel: string;
  state: WatchStateName;
  /** The row text, e.g. "Watching CI" or "CI passed after 1 fix". Set by the backend. */
  stateText: string;
  triesUsed: number;
  triesMax: number;
  lastFix: FixView | null;
  project: string;
  handle: string;
}

/** One fix try (`FixView`, camelCase). No log lines here. */
export interface FixView {
  id: string;
  watchId: string;
  tryNo: number;
  state: FixStateName;
  stateText: string;
  /** The failing check names, already cleaned. */
  checks: string[];
  notes: CiLogNotes;
  paneId: string | null;
  atMs: number;
}

/** Everything the Labs block shows (Rust `CiFixView`). */
export interface CiFixView {
  settings: CiFixSettings;
  gh: GhView;
  watches: WatchView[];
  /** The last 20 fixes, newest first. */
  fixes: FixView[];
}

/** The cleaned log a fix was started with (Rust `FixLogView`). Only the stored snapshot; empty once the fix ended. */
export interface FixLogView {
  checks: string[];
  /** Cleaned log lines, keyed by check name. */
  lines_by_check: Record<string, string[]>;
  notes: CiLogNotes;
  sha8: string;
}

/** The buttons the backend offers on a row or a card (`ci_fix_act`). */
export type CiFixAction = 'stop_watching' | 'watch_again' | 'ask_again' | 'push' | 'dont_push';

/** The Tauri event that says a watch or a fix changed. */
export const CI_FIX_CHANGED_EVENT = 'ci-fix://changed';

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return args === undefined ? invoke<T>(command) : invoke<T>(command, args);
}

const noop: UnlistenFn = () => {};

/** The Labs view: settings, `gh` state, watched pull requests and the last fixes. */
export const ciFixView = () => call<CiFixView>('ci_fix_view');

/** Saves the settings. The backend clamps them and returns the new view. */
export const ciFixSaveSettings = (settings: CiFixSettings) => call<CiFixView>('ci_fix_save_settings', { settings });

/** Watches the PR of this copy (if not watched) and raises the fix card when its head already failed. */
export const ciFixOffer = (projectPath: string | null, handle: string) =>
  call<WatchView>('ci_fix_offer', { projectPath, handle });

/** Runs one button of a row: stop or watch again (watch ids), or ask again, push, don't push (fix ids). */
export const ciFixAct = (id: string, action: CiFixAction) => call<CiFixView>('ci_fix_act', { id, action });

/** The cleaned log snapshot of one fix. */
export const ciFixLog = (fixId: string) => call<FixLogView>('ci_fix_log', { fixId });

/** Checks GitHub now instead of waiting for the next poll. */
export const ciFixPollNow = () => call<void>('ci_fix_poll_now');

/** Calls `cb` when a watch or a fix changes. Returns the unlisten function (a no-op outside the desktop shell). */
export async function onCiFixChanged(cb: () => void): Promise<UnlistenFn> {
  if (!isTauri()) return noop;
  return listen(CI_FIX_CHANGED_EVENT, () => cb());
}

/** "#61 acme/billing · Codex · Watching CI": one line for a watched pull request. */
export function watchLine(w: WatchView): string {
  return `#${w.prNumber} ${w.repo} · ${w.agentLabel} · ${w.stateText}`;
}

/** "200 log lines · 2 secrets hidden": what the card says about the log it will read. */
export function notesLine(notes: CiLogNotes): string {
  const parts = [`${notes.lines} log ${notes.lines === 1 ? 'line' : 'lines'}`];
  if (notes.hidden_secrets > 0) {
    parts.push(`${notes.hidden_secrets} ${notes.hidden_secrets === 1 ? 'secret' : 'secrets'} hidden`);
  }
  if (notes.invisible_chars > 0) {
    parts.push(
      `${notes.invisible_chars} invisible ${notes.invisible_chars === 1 ? 'character' : 'characters'} removed`,
    );
  }
  return parts.join(' · ');
}
