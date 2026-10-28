// Plain rules for the desktop companion screens (SPEC-1028 sections 1, 6, 8). Pure functions only: the cards and the
// sheets call them, and the tests pin them.
import type { CheckIn, CompanionPersona, CompanionView, HhMm, MemoryFile, When } from '../../../lib/companion';

/** A companion's pane is `companion-<id>` (SPEC section 4). Panes with this prefix are not listed as plain panes. */
export const COMPANION_PANE_PREFIX = 'companion-';

export const isCompanionPaneId = (id: string): boolean => id.startsWith(COMPANION_PANE_PREFIX);

/** The three memory files, in the order the card shows them. */
export const MEMORY_TABS: { file: MemoryFile; label: string; fileName: string }[] = [
  { file: 'soul', label: 'Who it is', fileName: 'SOUL.md' },
  { file: 'user', label: 'About you', fileName: 'USER.md' },
  { file: 'memory', label: 'Its notes', fileName: 'MEMORY.md' },
];

export const fileNameOf = (file: MemoryFile): string => MEMORY_TABS.find((t) => t.file === file)?.fileName ?? file;

/** The tabs of "What {name} knows": the three files, then what it has learned to do. */
export type MemoryTab = MemoryFile | 'skills';
export const KNOWS_TABS: { id: MemoryTab; label: string }[] = [
  ...MEMORY_TABS.map((t) => ({ id: t.file as MemoryTab, label: t.label })),
  { id: 'skills', label: 'Skills' },
];

/** The four presets from the setup sheet (step 4). */
export const PERSONA_OPTIONS: { id: Exclude<CompanionPersona, 'custom'>; label: string }[] = [
  { id: 'cheerful', label: 'Cheerful' },
  { id: 'calm', label: 'Calm and short' },
  { id: 'coach', label: 'Curious coach' },
  { id: 'plain', label: 'Plain and direct' },
];

/** Characters, as the Rust side counts them (Unicode scalars, not UTF-16 units or bytes). */
export const countChars = (text: string): number => Array.from(text).length;

/** `1,204`: a count with thousands separators, in the same form everywhere. */
export const formatCount = (n: number): string => n.toLocaleString('en-US');

/** A name is 1 to 24 characters with no control characters (SPEC section 2.1). */
export function isValidName(name: string): boolean {
  const trimmed = name.trim();
  return countChars(trimmed) >= 1 && countChars(trimmed) <= 24 && !/[\u0000-\u001f\u007f]/.test(trimmed);
}

/** The state to read on the card: the backend's sentence, or the plain word when it sent none. */
export const stateLineOf = (view: Pick<CompanionView, 'stateLine' | 'state'>, fallback: string): string => view.stateLine || fallback;

/** The check-in presets in the add sheet, in order. */
export type WhenKind = 'daily' | 'weekdays' | 'weekly' | 'every' | 'once';

export const WHEN_OPTIONS: { kind: WhenKind; label: string }[] = [
  { kind: 'daily', label: 'Every day' },
  { kind: 'weekdays', label: 'Weekdays' },
  { kind: 'weekly', label: 'Once a week' },
  { kind: 'every', label: 'Every N hours' },
  { kind: 'once', label: 'Once' },
];

export const DAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];

/** The fields the add sheet reads. Unused ones are ignored for the chosen kind. */
export interface WhenFields {
  at: HhMm;
  /** 0 = Monday .. 6 = Sunday. */
  day: number;
  /** Whole hours, 1 or more (the backend minimum is 30 minutes). */
  hours: number;
  /** Local date and time of a one-off check-in, in ms. */
  atMs: number;
}

/** Builds the `When` for a preset. `every` is in minutes; the backend refuses less than 30. */
export function buildWhen(kind: WhenKind, fields: WhenFields): When {
  switch (kind) {
    case 'daily':
      return { kind: 'daily', at: fields.at };
    case 'weekdays':
      return { kind: 'weekdays', at: fields.at };
    case 'weekly':
      return { kind: 'weekly', day: fields.day, at: fields.at };
    case 'every':
      return { kind: 'every', minutes: Math.max(1, Math.round(fields.hours)) * 60 };
    case 'once':
      return { kind: 'once', atMs: fields.atMs };
  }
}

/** The label is the first 40 characters of what to ask (the add sheet has no label field). */
export const checkInLabel = (text: string): string => Array.from(text.trim()).slice(0, 40).join('');

/** A new check-in id: 8 lowercase hex characters, as the backend expects. */
export function newCheckInId(random: () => number = Math.random): string {
  return Array.from({ length: 8 }, () => Math.floor(random() * 16).toString(16)).join('');
}

/** `08:00` from an `<input type="time">` value, or null when it is not a time. */
export function parseClock(value: string): HhMm | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  return h <= 23 && m <= 59 ? { h, m } : null;
}

const pad2 = (n: number): string => String(n).padStart(2, '0');

/** `17:00`: the local clock of a moment, in the form a time input takes. */
export function clockValue(ms: number): string {
  const at = new Date(ms);
  return `${pad2(at.getHours())}:${pad2(at.getMinutes())}`;
}

/** The next moment the local clock shows `at`: today when it is still ahead, else tomorrow. */
export function nextClockMs(at: HhMm, now: number): number {
  const day = new Date(now);
  day.setHours(at.h, at.m, 0, 0);
  if (day.getTime() <= now) day.setDate(day.getDate() + 1);
  return day.getTime();
}

/** `17:00` for a moment later today, `Thu 17:00` for another day. */
export function untilText(ms: number, now: number): string {
  const at = new Date(ms);
  const today = new Date(now);
  const sameDay = at.getFullYear() === today.getFullYear() && at.getMonth() === today.getMonth() && at.getDate() === today.getDate();
  return sameDay ? clockValue(ms) : `${at.toLocaleDateString('en-US', { weekday: 'short' })} ${clockValue(ms)}`;
}

/** The sentence under the state line while a move waits. */
export const movesWhenFree = (name: string): string => `Moves when ${name} is free`;

/** What the Setup card says about a companion that was moved: where it came from and when it goes back. */
export function movedNote(view: Pick<CompanionView, 'previousAgentLabel' | 'untilMs'>, now: number): string | null {
  if (!view.previousAgentLabel) return null;
  return view.untilMs
    ? `Moved from ${view.previousAgentLabel}. Goes back at ${untilText(view.untilMs, now)}.`
    : `Moved from ${view.previousAgentLabel}.`;
}

/** Characters in a skill file as the backend counts them: `# title`, a blank line, the steps and a final newline. */
export const skillFileChars = (title: string, body: string): number => countChars(title.trim()) + countChars(body.trim()) + 5;

/** The longest title (`SKILL_TITLE_CAP` in `companion/model.rs`). */
export const SKILL_TITLE_MAX = 60;
