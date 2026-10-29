// Focus: "press Focus, agents keep working, pings that can wait are held". The desktop side of the feature:
// the wire types (SPEC n33 section 3.5, mirrors the Rust `attention::focus::view`), the `focus_*` Tauri
// commands, the `focus://changed` event and the two small text helpers the note and the phone bar use.
// Rust serde: FocusView and PhoneFocus are camelCase at the top level; the records nested inside them keep
// the field names their Rust structs give them (snake_case), exactly as serialized.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the desktop listens to (payload: `FocusView`). */
export const FOCUS_EVENT = 'focus://changed';

/** The lengths the chips and the palette offer, in minutes. */
export const FOCUS_PRESETS = [25, 45, 90] as const;
export const FOCUS_MIN_MINUTES = 5;
export const FOCUS_MAX_MINUTES = 240;

/** `Settings` in Rust (`attention/focus/model.rs`). */
export interface FocusSettings {
  default_minutes: number;
  /** When false, cards that wait for the person go at normal strength during Focus. */
  quiet_blocking: boolean;
}

/** What Focus held (pings that could wait). Counts only. */
export interface HeldCounts {
  run_done: number;
  run_failed: number;
  companion: number;
  review: number;
  night: number;
}

export type FocusEndedBy = 'timer' | 'early';

export interface FocusCounts {
  turns_done: number;
  agents: number;
  failed: number;
  asked: number;
  urgent: number;
  still_waiting: number;
  held: HeldCounts;
}

/** `FocusSummary` in Rust: the one end-of-Focus summary. `line` is the sentence the person reads. */
export interface FocusSummary {
  session_id: string;
  started_at_ms: number;
  ended_at_ms: number;
  minutes: number;
  ended_by: FocusEndedBy;
  counts: FocusCounts;
  line: string;
}

/** `FocusView`: what the desktop shows (`focus_state`, the `focus://changed` payload). */
export interface FocusView {
  on: boolean;
  sessionId: string | null;
  startedAtMs: number | null;
  endsAtMs: number | null;
  minutes: number;
  extended: number;
  held: HeldCounts;
  heldTotal: number;
  waitingQuiet: number;
  waitingUrgent: number;
  settings: FocusSettings;
  lastSummary: FocusSummary | null;
}

/** The phone's end-of-Focus line. Counts and one sentence, no pane ids, no agent names. */
export interface PhoneFocusSummary {
  ended_at_ms: number;
  line: string;
  ended_by: FocusEndedBy;
}

/** `PhoneView`: the redacted view the phone reads from `GET /focus` (counts only). */
export interface PhoneFocus {
  on: boolean;
  endsAtMs: number | null;
  minutes: number;
  heldTotal: number;
  waiting: number;
  last: PhoneFocusSummary | null;
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Start a Focus (or change its length while on). The Rust side refuses a length outside 5..240. */
export const focusStart = (minutes: number): Promise<FocusView> => invoke<FocusView>('focus_start', { minutes });

/** Add 15 minutes to a Focus that is on. */
export const focusExtend = (): Promise<FocusView> => invoke<FocusView>('focus_extend');

/** End the Focus now (idempotent: while off this returns the off view). */
export const focusEnd = (): Promise<FocusView> => invoke<FocusView>('focus_end');

/** The current Focus view (also what the Settings and the note read on mount). */
export const focusState = (): Promise<FocusView> => invoke<FocusView>('focus_state');

/** Save the default length and the quiet switch. */
export const focusSettingsSet = (settings: FocusSettings): Promise<FocusView> =>
  invoke<FocusView>('focus_settings_set', { settings });

/** "Got it": clear the last summary and its pending digest. */
export const focusSummaryDismiss = (): Promise<void> => invoke<void>('focus_summary_dismiss');

/** Subscribes to `focus://changed`. Outside the desktop shell nothing is sent, so this is a no-op. */
export async function onFocusChanged(handler: (view: FocusView) => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => {};
  return listen<FocusView>(FOCUS_EVENT, (event) => handler(event.payload));
}

/**
 * "31 min left" from the end time. Whole minutes, rounded down, so the bar never promises more time than
 * is left. Under a minute (or past the end) reads "under 1 min left"; exactly one minute reads "1 min left".
 */
export function leftText(endsAtMs: number, now: number): string {
  const minutes = Math.floor((endsAtMs - now) / 60_000);
  if (minutes < 1) return 'under 1 min left';
  if (minutes === 1) return '1 min left';
  return `${minutes} min left`;
}

/** "Holding 3 updates" / "Holding 1 update" / "Nothing held yet", from the held total of a view. */
export function heldText(view: { heldTotal: number }): string {
  const n = view.heldTotal;
  if (n === 0) return 'Nothing held yet';
  return `Holding ${n} ${n === 1 ? 'update' : 'updates'}`;
}
