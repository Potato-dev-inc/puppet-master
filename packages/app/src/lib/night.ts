// Night Shift on the desktop: the view shapes (mirroring `night_shift/view.rs` and the camelCase structs in
// `commands/night.rs`), the 14 `night_*` commands, the `night://changed` event and the error helpers.
// Rust `Option<T>` arrives as `T | null`. Names and argument keys must match the Rust code exactly
// (`night.test.ts` checks them against `commands/night.rs`). Plain words follow SPEC n32 3.10.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the desktop listens to (`NIGHT_EVENT` in `commands/night.rs`). */
export const NIGHT_EVENT = 'night://changed';

/** `NightView.state` (`state_tag` in `view.rs`). */
export type NightRunState = 'scheduled' | 'running' | 'done' | 'stopped';

/**
 * The `state` in the `night://changed` payload is the raw `RunState`: unit variants are strings, and
 * `Scheduled` is an object with its field.
 */
export type NightRunStatePayload = 'running' | 'done' | 'stopped' | { scheduled: { start_at_ms: number } };

export interface NightChangedPayload {
  rev: number;
  state: NightRunStatePayload;
}

/** `TaskView.state` (`state_text` and `until` come with it). */
export type NightTaskState = 'queued' | 'waiting' | 'working' | 'finished';
/** `TaskView.class`: null until the task is classified. */
export type NightClass = 'ready' | 'no_checks' | 'needs_look' | 'no_changes' | 'didnt_run';
/** `TaskView.review`. */
export type NightReview = 'pending' | 'kept' | 'discarded' | 'keep_failed';
/** `AttemptView.end`. */
export type NightAttemptEnd =
  | 'running'
  | 'completed'
  | 'out_of_usage'
  | 'gate'
  | 'asked'
  | 'needs_you'
  | 'failed'
  | 'cut_off'
  | 'stopped_by_person';

export interface NightCounts {
  passed: number;
  total: number;
}

export interface NightFilesView {
  count: number;
  added: number;
  removed: number;
}

/** One try of a task, for "Tries" on the row. */
export interface NightAttemptView {
  n: number;
  label: string;
  end: NightAttemptEnd;
  /** Plain words for the end. Null on the bridge. */
  sentence: string | null;
  started: string;
  ended: string | null;
}

/** One line of "The night" timeline. */
export interface NightEventView {
  at: string;
  kind: string;
  glyph: string;
  text: string;
  task: string | null;
}

/** One task row on the Morning page. */
export interface NightTaskView {
  id: string;
  title: string;
  /** The folder name only. Null on the bridge. */
  project: string | null;
  plan: string | null;
  state: NightTaskState;
  stateText: string;
  until: string | null;
  class: NightClass | null;
  headline: string | null;
  reason: string | null;
  tests: NightCounts | null;
  testsBefore: NightCounts | null;
  testChangeLines: string[];
  files: NightFilesView | null;
  turns: number;
  duration: string | null;
  summaries: string[];
  attempts: NightAttemptView[];
  review: NightReview;
  reviewText: string | null;
  /** The branch a kept copy merged into. */
  into: string | null;
  canKeep: boolean;
  canThrowAway: boolean;
  /** The pane of the last try. Null on the bridge. */
  paneId: string | null;
}

/** How many tasks ended on one plan. */
export interface NightPlanCount {
  label: string;
  tasks: number;
}

/** The Morning page and the Tonight header for one run. */
export interface NightView {
  id: string;
  rev: number;
  state: NightRunState;
  line: string;
  total: number;
  done: number;
  green: number;
  needsYou: number;
  didntRun: number;
  noChanges: number;
  /** How many tasks "Keep all that pass" would keep. */
  keepAll: number;
  from: string;
  to: string | null;
  turns: number;
  duration: string;
  byPlan: NightPlanCount[];
  tasks: NightTaskView[];
  events: NightEventView[];
}

/** One installed plan (agent x login) as the Tonight sheet lists it. */
export interface NightPlanView {
  key: string;
  label: string;
  ready: boolean;
  whyNot: string | null;
  outUntil: string | null;
  /** A hint for "may ask you a question", never a promise. */
  triedHere: Record<string, boolean>;
}

export interface NightHistoryRow {
  id: string;
  line: string;
}

/** The Tonight settings, as the sheet reads and writes them. */
export interface NightSettingsView {
  plans: string[];
  stopHhmm: string;
  parallel: number;
  turns: number;
  minutes: number;
  awakeAsked: boolean;
}

/** What Tonight and Morning read when they open (`night_state`). */
export interface NightStateView {
  enabled: boolean;
  /** The current run, or the last one that finished. */
  run: NightView | null;
  canStart: boolean;
  /** Why Start is off right now, in plain words. */
  startBlocked: string | null;
  history: NightHistoryRow[];
  settings: NightSettingsView;
}

/** One declared check of a project, as the Tests popover shows it. */
export interface NightCheckView {
  name: string;
  argv: string[];
  timeoutMin: number;
}

/** A check Puppet Master found in the project. Only offered until the person saves it. */
export interface NightSuggestionView {
  name: string;
  argv: string[];
  source: string;
}

/** The Tests popover for one project (`night_checks_get`). */
export interface NightChecksView {
  declared: NightCheckView[];
  suggestions: NightSuggestionView[];
  warnings: string[];
  /** Always null in v1: the baseline is not run before a night. */
  baselineRed: boolean | null;
}

/** One check as the person typed it (`night_checks_set`). */
export interface NightCheckInput {
  name: string;
  command: string;
  timeoutMin: number;
}

/** One task as the Tonight sheet sends it. Fields with a serde default may be left out. */
export interface NightTaskInput {
  project: string;
  text: string;
  /** Null or left out = any ticked plan with usage left; otherwise one plan key. */
  who?: string | null;
  mayChangeTests?: boolean;
  turns?: number;
  minutes?: number;
}

/** What the Tonight sheet's Start sends (`night_start`). */
export interface NightStartInput {
  tasks: NightTaskInput[];
  /** Plan keys (`agent` or `agent@account`) in the person's order. */
  plans: string[];
  stopHhmm: string;
  /** Null or left out = start now. */
  startHhmm?: string | null;
  parallel: number;
  keepAwake: boolean;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

export interface NightApi {
  state: () => Promise<NightStateView>;
  plans: (projects: string[]) => Promise<NightPlanView[]>;
  checksGet: (project: string) => Promise<NightChecksView>;
  checksSet: (project: string, list: NightCheckInput[]) => Promise<NightCheckView[]>;
  start: (input: NightStartInput) => Promise<NightView>;
  stop: () => Promise<NightView>;
  keep: (taskId: string) => Promise<NightView>;
  throwAway: (taskId: string) => Promise<NightView>;
  keepAllReady: () => Promise<NightView>;
  seen: () => Promise<void>;
  reportHtml: () => Promise<string>;
  /** Writes the page to a file the person picked. Resolves to the path it was written to. */
  reportSave: (path: string) => Promise<string>;
  settingsGet: () => Promise<NightSettingsView>;
  settingsSet: (settings: NightSettingsView) => Promise<NightSettingsView>;
}

export const nightApi: NightApi = {
  state: () => call<NightStateView>('night_state'),
  plans: (projects) => call<NightPlanView[]>('night_plans', { projects }),
  checksGet: (project) => call<NightChecksView>('night_checks_get', { project }),
  checksSet: (project, list) => call<NightCheckView[]>('night_checks_set', { project, list }),
  start: (input) => call<NightView>('night_start', { input }),
  stop: () => call<NightView>('night_stop'),
  keep: (taskId) => call<NightView>('night_keep', { taskId }),
  throwAway: (taskId) => call<NightView>('night_throw_away', { taskId }),
  keepAllReady: () => call<NightView>('night_keep_all_ready'),
  seen: () => call<void>('night_seen'),
  reportHtml: () => call<string>('night_report_html'),
  reportSave: (path) => call<string>('night_report_save', { path }),
  settingsGet: () => call<NightSettingsView>('night_settings_get'),
  settingsSet: (settings) => call<NightSettingsView>('night_settings_set', { settings }),
};

/** Subscribes to `night://changed`. Outside the desktop shell nothing is sent, so this is a no-op. */
export async function onNightChanged(handler: (payload: NightChangedPayload) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<NightChangedPayload>(NIGHT_EVENT, (event) => handler(event.payload));
}

/** A refusal: a code the sheet can match, and the plain sentence to show. */
export interface NightError {
  code: string;
  message: string;
}

const CODE_PREFIX = /^([A-Z][A-Z0-9_]{2,}):\s*/;

function fromText(text: string): NightError {
  const match = CODE_PREFIX.exec(text);
  return match ? { code: match[1], message: text.slice(match[0].length) } : { code: '', message: text };
}

/**
 * The code and plain message of a failed Night Shift command. An `OperationError` arrives as the
 * object `{ code, message, recoverable, context }`; a plain error or string arrives as `CODE: message`.
 */
export function nightError(err: unknown): NightError {
  if (typeof err === 'object' && err !== null) {
    const { code, message } = err as { code?: unknown; message?: unknown };
    if (typeof code === 'string' && typeof message === 'string') return { code, message };
  }
  return fromText(err instanceof Error ? err.message : typeof err === 'string' ? err : String(err));
}

/** The plain sentence to show for a failed Night Shift command, without the code. */
export function nightErrorText(err: unknown): string {
  return nightError(err).message;
}

/** The code of a failed Night Shift command (e.g. `RUN_ACTIVE`), or an empty string when it has none. */
export function nightErrorCode(err: unknown): string {
  return nightError(err).code;
}
