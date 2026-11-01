// Routines of Potato Bot (phase P9a): a prompt a bot runs on a schedule. The names mirror
// src-tauri/src/potato/routines.rs and commands/potato_routines.rs in camelCase. Desktop only. Nothing here draws.
// `fakeRoutinesTransport` is the in-memory twin for tests (the pattern of `fakePotatoTransport`).
import { invoke } from '@tauri-apps/api/core';
import { isTauri } from './cockpit-api';
import type { ListStatus } from './potato-list';

/** When a routine runs (`RoutineSchedule` in routines.rs). Times are the computer's local clock. */
export type RoutineSchedule =
  | { kind: 'hourly' }
  | { kind: 'daily'; time: string }
  | { kind: 'weekdays'; time: string }
  | { kind: 'everyMinutes'; minutes: number }
  | { kind: 'once'; at: string };

export interface RoutineInput {
  title: string;
  prompt: string;
  schedule: RoutineSchedule;
}

export type RoutineStatus = 'active' | 'paused' | 'done';

/** One routine as the page shows it. `summary` is the schedule in words, made by the Rust side. */
export interface RoutineView {
  id: string;
  bot: string;
  title: string;
  prompt: string;
  schedule: RoutineSchedule;
  summary: string;
  status: RoutineStatus;
  paused: boolean;
  done: boolean;
  lastRunMs: number | null;
  nextRunMs: number | null;
}

/** What "Run now" answers: the brain got the prompt (or the note that it could not). */
export interface RoutineRunOutcome {
  id: string;
  delivered: boolean;
}

export interface RoutinesTransport {
  list(botId: string): Promise<RoutineView[]>;
  create(botId: string, input: RoutineInput): Promise<RoutineView>;
  pause(routineId: string, paused: boolean): Promise<RoutineView>;
  remove(routineId: string): Promise<void>;
  runNow(routineId: string): Promise<RoutineRunOutcome>;
}

/** Outside the desktop app (a browser preview, the tests) there are no routines to list, and nothing can change. */
const NOT_ON_DESKTOP = 'Routines need the Puppet Master desktop app.';

export const tauriRoutinesTransport: RoutinesTransport = {
  list: (botId) => (isTauri() ? invoke<RoutineView[]>('potato_routines_list', { botId }) : Promise.resolve([])),
  create: (botId, input) => (isTauri() ? invoke<RoutineView>('potato_routine_create', { botId, input }) : Promise.reject(new Error(NOT_ON_DESKTOP))),
  pause: (routineId, paused) => (isTauri() ? invoke<RoutineView>('potato_routine_pause', { routineId, paused }) : Promise.reject(new Error(NOT_ON_DESKTOP))),
  remove: (routineId) => (isTauri() ? invoke<void>('potato_routine_delete', { routineId }) : Promise.reject(new Error(NOT_ON_DESKTOP))),
  runNow: (routineId) => (isTauri() ? invoke<RoutineRunOutcome>('potato_routine_run_now', { routineId }) : Promise.reject(new Error(NOT_ON_DESKTOP))),
};

// ---- the form -----------------------------------------------------------------------------------

/** The choices of the "New routine" form. `every` is "Every few minutes". */
export type RoutineFreq = 'hourly' | 'daily' | 'weekdays' | 'every' | 'once';

export interface RoutineFormValues {
  title: string;
  prompt: string;
  freq: RoutineFreq;
  /** "HH:MM" for daily and weekdays. */
  time: string;
  /** For every few minutes: 15 or more. */
  minutes: number;
  /** "YYYY-MM-DDTHH:MM" for once (the value of a datetime-local input). */
  at: string;
}

export const DEFAULT_ROUTINE_FORM: RoutineFormValues = {
  title: '',
  prompt: '',
  freq: 'weekdays',
  time: '09:00',
  minutes: 30,
  at: '',
};

/** The request for the form's values, or one plain sentence that says what is missing. */
export function buildRoutineInput(values: RoutineFormValues): RoutineInput | string {
  const title = values.title.trim();
  const prompt = values.prompt.trim();
  if (!title) return 'Give the routine a name.';
  if (!prompt) return 'Write what the bot should do.';
  switch (values.freq) {
    case 'hourly':
      return { title, prompt, schedule: { kind: 'hourly' } };
    case 'daily':
    case 'weekdays':
      if (!/^\d{2}:\d{2}$/.test(values.time)) return 'Pick a time.';
      return { title, prompt, schedule: { kind: values.freq, time: values.time } };
    case 'every':
      if (!Number.isInteger(values.minutes) || values.minutes < 15) return 'Pick every 15 minutes or more.';
      return { title, prompt, schedule: { kind: 'everyMinutes', minutes: values.minutes } };
    case 'once':
      if (!values.at) return 'Pick a date and time.';
      return { title, prompt, schedule: { kind: 'once', at: values.at } };
  }
}

// ---- copy ---------------------------------------------------------------------------------------

/** "today at 9:00 AM", "tomorrow at 6:00 AM", "Nov 3 at 9:00 AM". */
export function nextRunWords(at: number, now: number): string {
  const when = new Date(at);
  const today = new Date(now);
  const tomorrow = new Date(now + 24 * 60 * 60 * 1000);
  const time = when.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
  if (when.toDateString() === today.toDateString()) return `today at ${time}`;
  if (when.toDateString() === tomorrow.toDateString()) return `tomorrow at ${time}`;
  return `${when.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })} at ${time}`;
}

/** The second line of a row: what happens next, or that it is paused or finished. */
export function routineStatusLine(routine: RoutineView, now: number): string {
  if (routine.status === 'done') return 'Ran once. Done.';
  if (routine.status === 'paused') return 'Paused';
  if (routine.nextRunMs == null) return 'No next run yet';
  return `Next run ${nextRunWords(routine.nextRunMs, now)}`;
}

export const ROUTINE_COPY = {
  title: 'Routines',
  empty: 'No routines yet. A routine is a job this bot runs on a schedule.',
  newRoutine: 'New routine',
  runNow: 'Run now',
  delete: 'Delete',
  confirmDelete: 'Delete this routine?',
  keep: 'Keep it',
  saving: 'Saving...',
  save: 'Save routine',
  cancel: 'Cancel',
  sent: (name: string) => `Sent to ${name}.`,
  loadFailed: 'Could not load the routines.',
  saveFailed: 'Could not save the routine.',
  actionFailed: 'That did not work. Try again.',
} as const;

// ---- the bot list's second line ------------------------------------------------------------------

/** The grey line under a bot's name in the list: what the bot waits for, does, or does next. */
export interface ListNextLine {
  /** `needs` gets the one accent dot; the rest are grey. */
  tone: 'needs' | 'working' | 'quiet';
  text: string;
}

export const LIST_NEXT_COPY = {
  waiting: 'Waiting for you',
  thinking: 'Thinking…',
  paused: 'Paused',
  next: (name: string, when: string) => `Next: ${name} · ${when}`,
} as const;

/**
 * The time a row shows for the next run: `9:00` today, `Tomorrow 9:00`, else the weekday `Tue 9:00`. The clock and the
 * weekday follow the locale (default: the person's).
 */
export function nextRunShort(at: number, now: number, locale?: string): string {
  const when = new Date(at);
  const today = new Date(now);
  const tomorrow = new Date(now + 24 * 60 * 60 * 1000);
  const time = when.toLocaleTimeString(locale, { hour: 'numeric', minute: '2-digit' });
  if (when.toDateString() === today.toDateString()) return time;
  if (when.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
  return `${when.toLocaleDateString(locale, { weekday: 'short' })} ${time}`;
}

/** The active routine that runs next, or null. Paused and finished routines do not count. */
export function nextRoutine(routines: readonly RoutineView[]): { title: string; at: number } | null {
  let best: { title: string; at: number } | null = null;
  for (const r of routines) {
    if (r.status !== 'active' || r.nextRunMs == null) continue;
    if (!best || r.nextRunMs < best.at) best = { title: r.title, at: r.nextRunMs };
  }
  return best;
}

/**
 * The second line of a row, by priority: waiting on the person, working, paused, then the next routine. Null means no
 * line: the row keeps its preview. Errors are not here (the row shows its own pill). "Using the computer" has no per-bot
 * signal on the client yet, so it is not chosen.
 */
export function listNextLine(status: ListStatus | null, routines: readonly RoutineView[], now: number, locale?: string): ListNextLine | null {
  if (status === 'needs') return { tone: 'needs', text: LIST_NEXT_COPY.waiting };
  if (status === 'working') return { tone: 'working', text: LIST_NEXT_COPY.thinking };
  if (status === 'paused') return { tone: 'quiet', text: LIST_NEXT_COPY.paused };
  if (status === 'error') return null;
  const next = nextRoutine(routines);
  if (!next) return null;
  return { tone: 'quiet', text: LIST_NEXT_COPY.next(next.title, nextRunShort(next.at, now, locale)) };
}

// ---- the fake -----------------------------------------------------------------------------------

export interface FakeRoutinesTransport extends RoutinesTransport {
  readonly calls: { method: keyof RoutinesTransport; args: unknown[] }[];
  /** The rows the fake holds, after the calls so far. */
  readonly routines: RoutineView[];
}

/** An in-memory `RoutinesTransport`. `fail` makes every call reject with that message. */
export function fakeRoutinesTransport(seed: RoutineView[] = [], fail?: string): FakeRoutinesTransport {
  const routines = [...seed];
  const calls: FakeRoutinesTransport['calls'] = [];
  let nextId = 1;
  const run = <T,>(method: keyof RoutinesTransport, args: unknown[], work: () => T): Promise<T> => {
    calls.push({ method, args });
    if (fail) return Promise.reject(new Error(fail));
    return Promise.resolve().then(work);
  };
  const find = (id: string) => {
    const routine = routines.find((r) => r.id === id);
    if (!routine) throw new Error('No routine has that name.');
    return routine;
  };
  return {
    calls,
    routines,
    list: (botId) => run('list', [botId], () => routines.filter((r) => r.bot === botId).map((r) => ({ ...r }))),
    create: (botId, input) =>
      run('create', [botId, input], () => {
        const made: RoutineView = {
          id: `r-fake-${nextId++}`,
          bot: botId,
          title: input.title,
          prompt: input.prompt,
          schedule: input.schedule,
          summary: input.schedule.kind,
          status: 'active',
          paused: false,
          done: false,
          lastRunMs: null,
          nextRunMs: null,
        };
        routines.push(made);
        return { ...made };
      }),
    pause: (routineId, paused) =>
      run('pause', [routineId, paused], () => {
        const routine = find(routineId);
        routine.paused = paused;
        routine.status = paused ? 'paused' : 'active';
        routine.nextRunMs = paused ? null : (routine.nextRunMs ?? Date.now() + 60 * 60 * 1000);
        return { ...routine };
      }),
    remove: (routineId) =>
      run('remove', [routineId], () => {
        const index = routines.findIndex((r) => r.id === routineId);
        if (index >= 0) routines.splice(index, 1);
      }),
    runNow: (routineId) =>
      run('runNow', [routineId], () => {
        find(routineId);
        return { id: routineId, delivered: true };
      }),
  };
}
