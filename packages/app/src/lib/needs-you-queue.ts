// The Needs you queue of the phone, from the PC's one attention list (pure helpers). The list is the
// queue: a blocking item leaves only by being resolved or expiring, and a resolved one stays as a
// tombstone for 15 s so a card that is on screen can say what happened. Nothing here drops a prompt.
import type { AttentionItem, AttentionSnapshot } from './attention';

/** The card of an answer THIS phone just gave leaves 3 s after it was answered (design 2.2). */
export const OWN_ANSWER_SHOWN_MS = 3_000;
/** A new card does not appear under a thumb that is touching the list: it waits this long after the touch ends. */
export const TOUCH_HOLD_MS = 600;

/** The `<subject>` of a `<kind>:<subject>` key: a question's id, a pane id. */
export function subjectOf(item: Pick<AttentionItem, 'key'>): string {
  const at = item.key.indexOf(':');
  return at < 0 ? item.key : item.key.slice(at + 1);
}

export const isBlocking = (item: AttentionItem): boolean => item.lane === 'blocking';
export const isOpen = (item: AttentionItem): boolean => item.state === 'open';

/** Kinds whose answer is a question card (choices and text come from `GET /questions`). */
export const QUESTION_KINDS: ReadonlyArray<AttentionItem['kind']> = ['question', 'consent', 'budget', 'limit'];
/** Kinds the card itself answers with the PC's fingerprint check. */
export const PROMPT_KINDS: ReadonlyArray<AttentionItem['kind']> = ['approval', 'trust_gate'];

/** Open blocking items in the PC's order (high risk and consent first, then oldest first), then the
 *  tombstones of what just left. `settledAt` (item id -> when this phone answered it) hides this
 *  phone's own answered cards 3 s after the answer. */
export function visibleQueue(snapshot: AttentionSnapshot, settledAt: ReadonlyMap<string, number>, now: number): AttentionItem[] {
  return snapshot.items.filter((item) => {
    if (!isBlocking(item)) return false;
    if (isOpen(item)) return true;
    const own = settledAt.get(item.id);
    return own === undefined || now - own < OWN_ANSWER_SHOWN_MS;
  });
}

/** The count the header shows: open blocking items. Tombstones never count. */
export const queueCount = (snapshot: AttentionSnapshot): number => snapshot.items.filter((i) => isBlocking(i) && isOpen(i)).length;

/** What the docked card of a pane shows: its own first open item (or a tombstone that just left), the
 *  number of OTHER open items waiting, and the first of them to offer as "Next". */
export function dockFor(snapshot: AttentionSnapshot, paneId: string, settledAt: ReadonlyMap<string, number>, now: number, shown: ReadonlySet<string> = new Set()) {
  const queue = visibleQueue(snapshot, settledAt, now);
  const mine = queue.filter((item) => item.pane?.id === paneId && PROMPT_KINDS.includes(item.kind));
  const current = mine.find(isOpen) ?? mine[0] ?? null;
  // `shown`: items already drawn elsewhere in the dock (a helper's card in its parent's chat) are not "more" or "Next".
  const others = queue.filter((item) => isOpen(item) && item !== current && !shown.has(item.id));
  return { current, more: others.length, next: others[0] ?? null };
}

/** The items to draw while a finger may be on the list: the ones already on screen keep updating in
 *  place, but an item that was not there before waits (`known` = the ids drawn so far). */
export function withoutNewItems(items: AttentionItem[], known: ReadonlySet<string>): AttentionItem[] {
  return items.filter((item) => known.has(item.id));
}
