// Pure rules of the Potato Bot page: what a thread turns into on screen, and which buttons a bot gets.
import type { BrainState, PotatoBotView, PotatoMessage } from '../../../lib/potato';
import { TIME_GAP_MS, clockLabel, dayLabel, sameDay, toolStep, type ToolStep } from '../../../lib/potato-thread';

export type ThreadRow =
  | { type: 'bubble'; message: PotatoMessage; side: 'you' | 'bot'; /** First bubble of a run by one sender. */ first: boolean }
  | { type: 'note'; message: PotatoMessage }
  | { type: 'card'; message: PotatoMessage; open: boolean };

/** Helper cards that nobody has answered: no `helper_result` row names their request. */
export function openCardIds(messages: readonly PotatoMessage[]): Set<string> {
  const closed = new Set(messages.filter((m) => m.kind === 'helper_result' && m.requestId).map((m) => m.requestId!));
  return new Set(messages.filter((m) => m.kind === 'helper_card' && m.requestId && !closed.has(m.requestId)).map((m) => m.requestId!));
}

/** The thread as rows. A reply with several segments is several bubbles; they sit close together. */
export function threadRows(messages: readonly PotatoMessage[]): ThreadRow[] {
  const open = openCardIds(messages);
  const rows: ThreadRow[] = [];
  let lastSide: 'you' | 'bot' | null = null;
  for (const message of messages) {
    if (message.kind === 'helper_card') {
      rows.push({ type: 'card', message, open: message.requestId ? open.has(message.requestId) : false });
      lastSide = null;
    } else if (message.kind === 'note' || message.kind === 'helper_result' || message.from.kind === 'note') {
      rows.push({ type: 'note', message });
      lastSide = null;
    } else {
      const side = message.from.kind === 'you' ? 'you' : 'bot';
      rows.push({ type: 'bubble', message, side, first: lastSide !== side });
      lastSide = side;
    }
  }
  return rows;
}

/** What the message list shows: the rows, plus the lines between them. */
export type ThreadItem =
  | ThreadRow
  | { type: 'day'; key: string; label: string }
  | { type: 'time'; key: string; label: string }
  | { type: 'unread'; key: string }
  | { type: 'activity'; key: string; steps: ToolStep[] };

export interface ThreadItemOptions {
  now: number;
  /** The first unread message: the "New" line goes above it. */
  unreadId?: string | null;
}

/**
 * The thread as it is drawn: a day line when the day changes (and at the top), a time line after a long pause, the
 * "New" line above the first unread message, and the bot's tool results folded into one activity row per run.
 * A bubble that follows any of those starts a new run of one sender.
 */
export function threadItems(messages: readonly PotatoMessage[], { now, unreadId = null }: ThreadItemOptions): ThreadItem[] {
  const items: ThreadItem[] = [];
  let prev: PotatoMessage | undefined;
  let group: Extract<ThreadItem, { type: 'activity' }> | null = null;
  let breakRun = false;
  let unreadPending = false;

  // The lines that go above a message: day, time, and the unread divider.
  const lead = (m: PotatoMessage) => {
    if (!prev || !sameDay(prev.at, m.at)) {
      items.push({ type: 'day', key: `day-${m.id}`, label: dayLabel(m.at, now) });
      items.push({ type: 'time', key: `time-${m.id}`, label: clockLabel(m.at) });
      breakRun = true;
    } else if (m.at - prev.at >= TIME_GAP_MS) {
      items.push({ type: 'time', key: `time-${m.id}`, label: clockLabel(m.at) });
      breakRun = true;
    }
    if (unreadPending) {
      items.push({ type: 'unread', key: `unread-${m.id}` });
      unreadPending = false;
      breakRun = true;
    }
  };

  for (const row of threadRows(messages)) {
    const m = row.message;
    if (m.id === unreadId) unreadPending = true;
    const step = row.type === 'bubble' && row.side === 'bot' ? toolStep(m) : null;
    if (step) {
      if (group) {
        group.steps.push(step);
      } else {
        lead(m);
        group = { type: 'activity', key: `activity-${m.id}`, steps: [step] };
        items.push(group);
        breakRun = true;
      }
      prev = m;
      continue;
    }
    group = null;
    lead(m);
    if (row.type === 'bubble') {
      items.push(breakRun && !row.first ? { ...row, first: true } : row);
      breakRun = false;
    } else {
      items.push(row);
    }
    prev = m;
  }
  if (unreadPending) items.push({ type: 'unread', key: 'unread-end' });
  return items;
}

/** Which of Stop and Start the header offers, if either. */
export function runButton(state: BrainState): 'stop' | 'start' | null {
  switch (state) {
    case 'ready':
    case 'starting':
    case 'restarting':
      return 'stop';
    case 'idle':
    case 'stopped':
      return 'start';
    default:
      return null;
  }
}

/** Whether the person can still write (a switched-off bot cannot). */
export function canWrite(state: BrainState): boolean {
  return state !== 'off';
}

/** Total unread over every bot: the number on the sidebar row. */
export function totalUnread(bots: readonly Pick<PotatoBotView, 'unread'>[]): number {
  return bots.reduce((sum, b) => sum + b.unread, 0);
}

/** The bot whose thread is open: the remembered one if it still exists, else the first. */
export function pickOpenBot<T extends { id: string }>(bots: readonly T[], openId: string | null): T | null {
  return bots.find((b) => b.id === openId) ?? bots[0] ?? null;
}
