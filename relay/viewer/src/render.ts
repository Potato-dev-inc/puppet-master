// Turns decrypted frames into one model and draws it. The model part is pure (no DOM), so the rules can be
// tested on their own. Every string goes into the page with textContent, so a chat text that looks like
// markup stays plain text.

import type { Frame, ItemView, LiveState, PaneView, RoomInfo, TaskView } from './types';

/** What the top of the page says about the connection. */
export type Status = 'connecting' | 'live' | 'reconnecting' | 'full' | 'ended' | 'broken';

export interface Model {
  status: Status;
  room: RoomInfo | null;
  panes: PaneView[];
  items: ItemView[];
}

/** Bounds the DOM: only the newest rows of each pane are kept. */
export const MAX_ITEMS_PER_PANE = 200;

const LIVE_STATES: readonly LiveState[] = ['working', 'idle', 'waiting', 'error', 'gone'];
const ITEM_KINDS: readonly string[] = ['user', 'agent', 'tool', 'result', 'end', 'plan', 'note'];

export function emptyModel(): Model {
  return { status: 'connecting', room: null, panes: [], items: [] };
}

function str(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function num(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0;
}

function normalizeRoom(raw: unknown): RoomInfo | null {
  if (typeof raw !== 'object' || raw === null) {
    return null;
  }
  const r = raw as Record<string, unknown>;
  return {
    title: str(r.title),
    endsAt: num(r.endsAt),
    paused: r.paused === true,
    hidden: num(r.hidden),
  };
}

function normalizePane(raw: unknown): PaneView | null {
  if (typeof raw !== 'object' || raw === null) {
    return null;
  }
  const p = raw as Record<string, unknown>;
  const key = str(p.key);
  if (!key) {
    return null;
  }
  const state = LIVE_STATES.includes(p.state as LiveState) ? (p.state as LiveState) : 'idle';
  return {
    key,
    label: str(p.label),
    title: str(p.title),
    state,
    line: typeof p.line === 'string' ? p.line : null,
  };
}

function normalizeTasks(raw: unknown): TaskView[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const tasks: TaskView[] = [];
  for (const task of raw.slice(0, 20)) {
    if (typeof task === 'object' && task !== null) {
      const t = task as Record<string, unknown>;
      tasks.push({ text: str(t.text), status: typeof t.status === 'string' ? t.status : null });
    }
  }
  return tasks;
}

/** One chat row, or null when the row is not one this page knows (it is skipped, never shown raw). */
export function normalizeItem(raw: unknown): ItemView | null {
  if (typeof raw !== 'object' || raw === null) {
    return null;
  }
  const r = raw as Record<string, unknown>;
  if (typeof r.t !== 'string' || !ITEM_KINDS.includes(r.t) || typeof r.id !== 'number') {
    return null;
  }
  const base = { id: r.id, pane: str(r.pane) };
  switch (r.t) {
    case 'user':
      return { t: 'user', ...base, text: str(r.text) };
    case 'agent':
      return { t: 'agent', ...base, text: str(r.text) };
    case 'tool':
      return {
        t: 'tool',
        ...base,
        name: str(r.name),
        summary: str(r.summary),
        files: num(r.files),
        writes: r.writes === true,
      };
    case 'result':
      return { t: 'result', ...base, ok: r.ok === true, lines: num(r.lines) };
    case 'end':
      return {
        t: 'end',
        ...base,
        outcome: str(r.outcome),
        ms: typeof r.ms === 'number' ? r.ms : null,
      };
    case 'plan':
      return { t: 'plan', ...base, tasks: normalizeTasks(r.tasks), replace: r.replace === true };
    case 'note':
      return { t: 'note', ...base, text: str(r.text) };
    default:
      return null;
  }
}

/** Adds one item. A repeated id in the same pane is skipped; a plan with `replace` drops older plans. */
function addItem(model: Model, item: ItemView): void {
  if (model.items.some((old) => old.id === item.id && old.pane === item.pane)) {
    return;
  }
  if (item.t === 'plan' && item.replace) {
    model.items = model.items.filter((old) => !(old.t === 'plan' && old.pane === item.pane));
  }
  model.items.push(item);
  const samePane = model.items.filter((old) => old.pane === item.pane);
  if (samePane.length > MAX_ITEMS_PER_PANE) {
    const oldest = samePane[0];
    model.items = model.items.filter((old) => old !== oldest);
  }
}

/**
 * Applies one decrypted frame. A snap replaces everything, a delta adds items, a state changes only the
 * room and panes. Panes are merged by key, so a delta can name only the panes that changed.
 */
export function applyFrame(model: Model, frame: Frame): void {
  if (frame.kind === 'snap') {
    model.panes = [];
    model.items = [];
  }
  const room = normalizeRoom(frame.room);
  if (room) {
    model.room = room;
  }
  for (const raw of frame.panes ?? []) {
    const pane = normalizePane(raw);
    if (!pane) {
      continue;
    }
    const at = model.panes.findIndex((old) => old.key === pane.key);
    if (at >= 0) {
      model.panes[at] = pane;
    } else {
      model.panes.push(pane);
    }
  }
  for (const raw of frame.items ?? []) {
    const item = normalizeItem(raw);
    if (item) {
      addItem(model, item);
    }
  }
  if (model.status === 'connecting' || model.status === 'reconnecting') {
    model.status = 'live';
  }
}

/** The room is over: nothing stays on the page but the sentence. */
export function endModel(model: Model): void {
  model.status = 'ended';
  model.room = null;
  model.panes = [];
  model.items = [];
}

/** The one sentence under the header, or '' when there is nothing to say. */
export function noticeFor(model: Model): string {
  switch (model.status) {
    case 'connecting':
      return 'Connecting…';
    case 'reconnecting':
      return 'Connection lost. Trying again…';
    case 'full':
      return 'This Live Room is full';
    case 'ended':
      return 'This Live Room has ended';
    case 'broken':
      return 'This link does not work';
    default:
      break;
  }
  if (model.room?.paused) {
    return 'Paused by the owner';
  }
  if (model.room && model.room.hidden > 0) {
    return 'Some steps were not shared';
  }
  return '';
}

function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? '' : 's'}`;
}

/** "81 s" or "1 min 21 s"; '' when the owner sent no duration. */
export function durationLabel(ms: number | null): string {
  if (ms === null) {
    return '';
  }
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) {
    return `${seconds} s`;
  }
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

/** The right side of a tool row: "2 files · writes". */
export function toolMeta(item: Extract<ItemView, { t: 'tool' }>): string {
  const parts: string[] = [];
  if (item.files > 0) {
    parts.push(plural(item.files, 'file'));
  }
  if (item.writes) {
    parts.push('writes');
  }
  return parts.join(' · ');
}

/** The line of a result row: "failed · 37 lines". */
export function resultLabel(item: Extract<ItemView, { t: 'result' }>): string {
  return `${item.ok ? 'ok' : 'failed'} · ${plural(item.lines, 'line')}`;
}

/** A pane state as a person reads it. */
export function stateWord(state: LiveState): string {
  switch (state) {
    case 'working':
      return 'Working';
    case 'waiting':
      return 'Waiting';
    case 'error':
      return 'Error';
    case 'gone':
      return 'Closed';
    default:
      return 'Idle';
  }
}
