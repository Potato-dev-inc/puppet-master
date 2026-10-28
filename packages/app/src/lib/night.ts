// Night Shift on the desktop: the view shapes (mirroring `night_shift/view.rs` and the camelCase structs in
// `commands/night.rs`), the 15 `night_*` commands, the `night://changed` event and the error helpers.
// Rust `Option<T>` arrives as `T | null`. Names and argument keys must match the Rust code exactly
// (`night.test.ts` checks them against `commands/night.rs`). Plain words follow SPEC n32 3.10.
import { useCallback, useEffect, useRef, useState } from 'react';
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
  /** One run by id (the Morning page opens a past night with it). */
  get: (id: string) => Promise<NightView>;
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
  get: (id) => call<NightView>('night_get', { id }),
  seen: () => call<void>('night_seen'),
  reportHtml: () => call<string>('night_report_html'),
  reportSave: (path) => call<string>('night_report_save', { path }),
  settingsGet: () => call<NightSettingsView>('night_settings_get'),
  settingsSet: (settings) => call<NightSettingsView>('night_settings_set', { settings }),
};

/** One run by id: `night_get` with `{ id }`. */
export const nightGet = (id: string): Promise<NightView> => nightApi.get(id);

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

/** The sentence for Labs off (the sheet shows it on its disabled Start too). */
export const LABS_OFF_SENTENCE = 'A person can turn Night Shift on in Settings > General > Labs.';
/** The sentence for an unfinished run (the sheet shows it on its disabled Start too). */
export const BUSY_SENTENCE = "Finish or stop tonight's run first.";

// The limits of `night_shift/model.rs` (the sheet refuses the same values).
const MAX_TASKS = 10;
const MAX_TASK_TEXT = 2000;
const MIN_TURNS = 3;
const MAX_TURNS = 40;
const MIN_MINUTES = 20;
const MAX_MINUTES = 240;
const DEFAULT_TURNS = 12;
const DEFAULT_MINUTES = 90;
const MIN_RUN_MS = 30 * 60_000;
const MAX_RUN_MS = 14 * 3_600_000;

/** What Start is checked against: the Labs switch, the run, the plans the sheet lists and the draft. */
export interface StartCheck {
  /** The Labs switch (`night_state.enabled`). */
  enabled: boolean;
  /** An unfinished run exists (`night_state.run` is scheduled or running). */
  busy: boolean;
  /** Undo is on and git is there. Default: true. */
  undoOn?: boolean;
  /** Whether a folder is a git project with a commit. Default: every folder passes. */
  isGit?: (project: string) => boolean;
  /** The plans the sheet lists (`night_plans`). */
  plans: NightPlanView[];
  /** The person has already confirmed that the computer may stay awake (`night_settings.awakeAsked`). */
  awakeAsked: boolean;
  input: NightStartInput;
  /** The clock, epoch ms. A start with no time means now. */
  nowMs: number;
}

const HHMM = /^(\d{2}):(\d{2})$/;

/** The first local `HH:MM` at or after `fromMs`, or null when the text is not a time. Same rule as `at_local`. */
function atLocal(fromMs: number, hhmm: string): number | null {
  const match = HHMM.exec(hhmm);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return null;
  const from = new Date(fromMs);
  for (let days = 0; days < 2; days += 1) {
    const at = new Date(from.getFullYear(), from.getMonth(), from.getDate() + days, hour, minute).getTime();
    if (at >= fromMs) return at;
  }
  return null;
}

/**
 * The first reason Start cannot run, as the code and the sentence the sheet shows, or null when it can.
 * Checks run in the same order as `commands/night/start.rs` `build`. Two checks the desktop makes that the
 * sheet cannot see are not here: two logins of one company (the plan list has no company) and an undo-less
 * folder (`undoOn` is for the caller to pass in). The desktop refuses both again.
 */
export function startBlockedReason(check: StartCheck): NightError | null {
  const fail = (code: string, message: string): NightError => ({ code, message });
  if (!check.enabled) return fail('NIGHT_SHIFT_OFF', LABS_OFF_SENTENCE);
  if (check.busy) return fail('RUN_ACTIVE', BUSY_SENTENCE);
  if (check.undoOn === false) {
    return fail(
      'MISSION_UNDO_OFF',
      'Missions need Undo to go back when a turn makes things worse. Undo is off for this folder.',
    );
  }
  const { input } = check;
  if (input.tasks.length === 0) return fail('NO_TASKS', 'Add at least one task.');
  if (input.tasks.length > MAX_TASKS) return fail('TOO_MANY_TASKS', `Night Shift takes up to ${MAX_TASKS} tasks.`);

  const keys: string[] = [];
  for (const key of input.plans) {
    if (keys.includes(key)) continue;
    const plan = check.plans.find((listed) => listed.key === key);
    if (!plan) return fail('PLAN_UNKNOWN', 'That plan is not on this computer. Pick it again under Plans.');
    if (!plan.ready) return fail('PLAN_NOT_READY', plan.whyNot ?? 'That plan is not ready.');
    keys.push(key);
  }
  if (keys.length === 0) return fail('NO_PLAN', 'Tick at least one plan to use.');

  const start = input.startHhmm == null ? check.nowMs : atLocal(check.nowMs, input.startHhmm);
  if (start === null) return fail('BAD_TIME', 'Pick a time like 23:30.');
  const stop = atLocal(start, input.stopHhmm);
  if (stop === null) return fail('BAD_TIME', 'Pick a time like 07:00.');
  if (stop - start < MIN_RUN_MS) return fail('STOP_TOO_SOON', 'Pick a stop time at least 30 minutes after the start.');
  if (stop - start > MAX_RUN_MS) return fail('STOP_TOO_LATE', 'Pick a stop time within 14 hours of the start.');

  if (input.keepAwake && !check.awakeAsked) {
    return fail('AWAKE_NOT_CONFIRMED', 'Confirm that the computer may stay awake first.');
  }

  for (const [index, task] of input.tasks.entries()) {
    const n = index + 1;
    const project = task.project.trim();
    if (!project) return fail('NO_PROJECT', `Pick a project for task ${n}.`);
    if (check.isGit && !check.isGit(project)) {
      return fail(
        'MISSION_NOT_GIT',
        `Task ${n}: this project is not a git project with a commit. Night Shift works in a separate copy of it.`,
      );
    }
    const text = task.text.trim();
    if (!text) return fail('TASK_TEXT_EMPTY', `Task ${n} needs words: what should it do?`);
    if ([...text].length > MAX_TASK_TEXT) {
      return fail('TASK_TEXT_TOO_LONG', `Task ${n} is longer than ${MAX_TASK_TEXT} characters.`);
    }
    const turns = task.turns ?? DEFAULT_TURNS;
    const minutes = task.minutes ?? DEFAULT_MINUTES;
    if (turns < MIN_TURNS || turns > MAX_TURNS || minutes < MIN_MINUTES || minutes > MAX_MINUTES) {
      return fail(
        'BUDGET_RANGE',
        `Task ${n}: turns must be ${MIN_TURNS} to ${MAX_TURNS}, and minutes ${MIN_MINUTES} to ${MAX_MINUTES}.`,
      );
    }
    const who = (task.who ?? '').trim();
    if (who !== '' && who !== 'any' && !keys.includes(who)) {
      return fail('AGENT_NOT_TICKED', `Task ${n} uses a plan that is not ticked under Plans.`);
    }
  }
  return null;
}

/** The counts the summary line is made of (`report::Summary`). */
export interface NightSummaryCounts {
  total: number;
  done: number;
  green: number;
  needsYou: number;
  didntRun: number;
  noChanges: number;
}

/** "4 of 5 done, tests green. 1 needs you." The same words as `report::line`. */
export function summaryLine(s: NightSummaryCounts): string {
  let out = `${s.done} of ${s.total} done`;
  if (s.done > 0) {
    if (s.green === s.done) out += ', tests green';
    else if (s.green > 0) out += `, tests green on ${s.green}`;
    else out += ', no tests ran';
  }
  if (s.needsYou === 1) out += '. 1 needs you.';
  else if (s.needsYou > 1) out += `. ${s.needsYou} need you.`;
  // The last two clauses end a sentence of their own. When no earlier clause ended one, start with a full stop.
  const tail: Array<[number, string]> = [
    [s.didntRun, "didn't run"],
    [s.noChanges, 'changed nothing'],
  ];
  for (const [n, text] of tail) {
    if (n > 0) {
      out += out.endsWith('.') ? ' ' : '. ';
      out += `${n} ${text}.`;
    }
  }
  return out;
}

/** What `useNight` gives the page. `error` is the plain sentence of the last failed read. */
export interface NightHook {
  /** The last state read, or null before the first read (and outside the desktop shell). */
  state: NightStateView | null;
  error: string | null;
  /** Reads the state again now. */
  reload: () => Promise<void>;
}

/** How often a running night is read again, as a fallback when an event is missed. */
export const NIGHT_POLL_MS = 30_000;

/** The current night: read on mount, again on every `night://changed`, and every 30 s while it runs. */
export function useNight(): NightHook {
  const [state, setState] = useState<NightStateView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const live = useRef(false);

  const reload = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const next = await nightApi.state();
      if (live.current) {
        setState(next);
        setError(null);
      }
    } catch (err) {
      if (live.current) setError(nightErrorText(err));
    }
  }, []);

  useEffect(() => {
    live.current = true;
    void reload();
    let disposed = false;
    let off: UnlistenFn | undefined;
    onNightChanged(() => void reload())
      .then((unlisten) => (disposed ? unlisten() : (off = unlisten)))
      .catch(() => {});
    return () => {
      live.current = false;
      disposed = true;
      off?.();
    };
  }, [reload]);

  const running = state?.run?.state === 'running';
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => void reload(), NIGHT_POLL_MS);
    return () => clearInterval(timer);
  }, [running, reload]);

  return { state, error, reload };
}

/** The Labs switch (`night_state.enabled`), read on mount and again when Settings change. */
export function useNightEnabled(): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let off: UnlistenFn | undefined;
    const read = () =>
      nightApi
        .state()
        .then((s) => !disposed && setOn(s.enabled))
        .catch(() => {});
    void read();
    listen('settings://changed', () => void read())
      .then((unlisten) => (disposed ? unlisten() : (off = unlisten)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return on;
}
