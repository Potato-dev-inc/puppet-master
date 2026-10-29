// Dev harness answers for the `focus_*` commands (lib/focus.ts). The mock keeps its own small state, so the note
// chips, the sidebar pill and the palette really start, extend and end a Focus in the browser.
import type { FocusSettings, FocusSummary, FocusView } from '../lib/focus';

export interface FocusScenario {
  /** Start with a Focus running (default: off). */
  on?: boolean;
  /** Minutes left of the running Focus (default 31). */
  minutesLeft?: number;
  /** What Focus holds right now (default: 3 updates). */
  heldTotal?: number;
  /** Cards that asked for the person during this Focus (default 1 quiet, 0 urgent). */
  waitingQuiet?: number;
  waitingUrgent?: number;
  /** The summary of the Focus that just ended, shown until "Got it". */
  lastSummary?: FocusSummary;
}

const SETTINGS: FocusSettings = { default_minutes: 45, quiet_blocking: true };

export const EXAMPLE_FOCUS_SUMMARY: FocusSummary = {
  session_id: 'fc-0123456789ab',
  started_at_ms: 0,
  ended_at_ms: 0,
  minutes: 45,
  ended_by: 'timer',
  counts: {
    turns_done: 4,
    agents: 2,
    failed: 1,
    asked: 2,
    urgent: 0,
    still_waiting: 1,
    held: { run_done: 2, run_failed: 1, companion: 0, review: 0, night: 0 },
  },
  line: 'While you focused (45 min): 4 turns done, 1 failed, 2 asked for you. 1 still waits.',
};

function held(total: number): FocusView['held'] {
  return { run_done: total, run_failed: 0, companion: 0, review: 0, night: 0 };
}

/** The view for a Focus that ends `minutes` from `now`. */
export function onView(s: FocusScenario | undefined, now: number, minutes = 45, left = s?.minutesLeft ?? 31): FocusView {
  const total = s?.heldTotal ?? 3;
  return {
    on: true,
    sessionId: 'fc-0123456789ab',
    startedAtMs: now - (minutes - left) * 60_000,
    endsAtMs: now + left * 60_000,
    minutes,
    extended: 0,
    held: held(total),
    heldTotal: total,
    waitingQuiet: s?.waitingQuiet ?? 1,
    waitingUrgent: s?.waitingUrgent ?? 0,
    settings: SETTINGS,
    lastSummary: null,
  };
}

export function offView(last: FocusSummary | null = null, settings: FocusSettings = SETTINGS): FocusView {
  return {
    on: false,
    sessionId: null,
    startedAtMs: null,
    endsAtMs: null,
    minutes: settings.default_minutes,
    extended: 0,
    held: held(0),
    heldTotal: 0,
    waitingQuiet: 0,
    waitingUrgent: 0,
    settings,
    lastSummary: last,
  };
}

/** One mock per harness page: keeps the Focus state between commands. `handled: false` leaves the command alone. */
export function createFocusMock(s: FocusScenario | undefined): (cmd: string, args: Record<string, unknown>) => { handled: boolean; value?: unknown } {
  let settings = SETTINGS;
  let view: FocusView = s?.on ? onView(s, Date.now()) : offView(s?.lastSummary ?? null);
  const set = (next: FocusView) => {
    view = next;
    return { handled: true, value: view };
  };
  return (cmd, args) => {
    if (!cmd.startsWith('focus_')) return { handled: false };
    const now = Date.now();
    switch (cmd) {
      case 'focus_state':
        return { handled: true, value: view };
      case 'focus_start': {
        const minutes = Number(args.minutes);
        if (!(minutes >= 5 && minutes <= 240)) throw new Error('FOCUS_BAD_MINUTES: Pick between 5 and 240 minutes.');
        return set({ ...onView({ heldTotal: view.heldTotal, waitingQuiet: view.waitingQuiet, waitingUrgent: view.waitingUrgent }, now, minutes, minutes), settings });
      }
      case 'focus_extend':
        if (!view.on || view.endsAtMs === null) throw new Error('FOCUS_NOT_ON: Focus is not on.');
        return set({ ...view, endsAtMs: view.endsAtMs + 15 * 60_000, extended: view.extended + 1 });
      case 'focus_end':
        return set(offView(view.on ? { ...EXAMPLE_FOCUS_SUMMARY, started_at_ms: view.startedAtMs ?? now, ended_at_ms: now } : view.lastSummary, settings));
      case 'focus_settings_set':
        settings = args.settings as FocusSettings;
        return set({ ...view, settings, minutes: view.on ? view.minutes : settings.default_minutes });
      case 'focus_summary_dismiss':
        view = { ...view, lastSummary: null };
        return { handled: true, value: null };
      default:
        return { handled: false };
    }
  };
}
