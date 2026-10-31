// Pure rules of the Potato Bot page: what a thread turns into on screen, and which buttons a bot gets.
import type { BrainState, PotatoBotView, PotatoMessage } from '../../../lib/potato';

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
