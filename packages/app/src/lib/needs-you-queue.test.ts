import { describe, expect, it } from 'vitest';
import { ITEM_ERROR, ITEM_HIGH, ITEM_LOW, ITEM_MEDIUM, ITEM_QUESTION, tombstone } from '../dev/attention-fixtures';
import type { AttentionItem, AttentionSnapshot } from './attention';
import { OWN_ANSWER_SHOWN_MS, dockFor, queueCount, subjectOf, visibleQueue, withoutNewItems } from './needs-you-queue';

const snap = (items: AttentionItem[]): AttentionSnapshot => ({
  v: 1,
  rev: 1,
  counts: { blocking: items.filter((i) => i.state === 'open' && i.lane === 'blocking').length, ready: 0 },
  items,
});
const NOW = Date.now();
const none = new Map<string, number>();

describe('the queue is the PC’s list', () => {
  it('keeps every open blocking item in the PC’s order and never drops one', () => {
    const items = [ITEM_HIGH, ITEM_MEDIUM, ITEM_LOW, ITEM_QUESTION, ITEM_ERROR];
    expect(visibleQueue(snap(items), none, NOW).map((i) => i.id)).toEqual(items.map((i) => i.id));
    expect(queueCount(snap(items))).toBe(5);
  });

  it('a hundred open items are a hundred cards', () => {
    const many = Array.from({ length: 100 }, (_, n) => ({ ...ITEM_LOW, id: `id${n}`, key: `approval:p${n}` }));
    expect(visibleQueue(snap(many), none, NOW)).toHaveLength(100);
  });

  it('leaves the ready lane out of the queue and out of the count', () => {
    const ready: AttentionItem = { ...ITEM_LOW, id: 'done1', key: 'done:p', kind: 'done', lane: 'ready' };
    expect(visibleQueue(snap([ITEM_LOW, ready]), none, NOW).map((i) => i.id)).toEqual([ITEM_LOW.id]);
    expect(queueCount(snap([ITEM_LOW, ready]))).toBe(1);
  });

  it('a tombstone stays for the card to say what happened, and never counts', () => {
    const gone = tombstone(ITEM_LOW, { state: 'gone', reason: 'moved_on' });
    const s = snap([ITEM_HIGH, gone]);
    expect(visibleQueue(s, none, NOW).map((i) => i.state)).toEqual(['open', 'resolved']);
    expect(queueCount(s)).toBe(1);
  });

  it('the card of an answer this phone gave leaves three seconds later, but an open one never does', () => {
    const own = tombstone(ITEM_LOW, { state: 'answered', by: 'phone', choice_label: 'Allowed once' });
    const answeredAt = new Map([[own.id, NOW]]);
    expect(visibleQueue(snap([own]), answeredAt, NOW + OWN_ANSWER_SHOWN_MS - 1)).toHaveLength(1);
    expect(visibleQueue(snap([own]), answeredAt, NOW + OWN_ANSWER_SHOWN_MS)).toHaveLength(0);
    // The PC has not resolved it yet: it is still waiting, so it stays however long ago the tap was.
    expect(visibleQueue(snap([ITEM_LOW]), new Map([[ITEM_LOW.id, NOW - 60_000]]), NOW)).toHaveLength(1);
    // Someone else’s answer stays as long as the PC keeps it.
    expect(visibleQueue(snap([own]), none, NOW + 14_000)).toHaveLength(1);
  });

  it('reads the subject of a key: a question id, a pane id', () => {
    expect(subjectOf({ key: 'question:q-1' })).toBe('q-1');
    expect(subjectOf({ key: 'approval:pane:with:colons' })).toBe('pane:with:colons');
  });
});

describe('the docked card', () => {
  it('shows the open pane’s own item, how many others wait, and the first of them as next', () => {
    const s = snap([ITEM_HIGH, ITEM_LOW, ITEM_QUESTION, ITEM_ERROR]);
    const dock = dockFor(s, ITEM_LOW.pane!.id, none, NOW);
    expect(dock.current?.id).toBe(ITEM_LOW.id);
    expect(dock.more).toBe(3);
    expect(dock.next?.id).toBe(ITEM_HIGH.id);
  });

  it('a pane with nothing of its own still tells how many wait', () => {
    const dock = dockFor(snap([ITEM_HIGH, ITEM_LOW]), 'pane-opencode-1', none, NOW);
    expect(dock.current).toBeNull();
    expect(dock.more).toBe(2);
  });

  it('keeps showing the item that just left (its tombstone) instead of swapping the card away', () => {
    const answered = tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' });
    const dock = dockFor(snap([ITEM_HIGH, answered]), ITEM_LOW.pane!.id, none, NOW);
    expect(dock.current?.state).toBe('resolved');
    expect(dock.more).toBe(1);
  });

  it('never swaps to another pane’s card by itself', () => {
    const dock = dockFor(snap([ITEM_HIGH]), ITEM_LOW.pane!.id, none, NOW);
    expect(dock.current).toBeNull();
    expect(dock.next?.id).toBe(ITEM_HIGH.id);
  });
});

describe('a new card does not move under a thumb', () => {
  it('holds back an item that was not on screen and keeps updating the ones that were', () => {
    const known = new Set([ITEM_HIGH.id]);
    const updated = { ...ITEM_HIGH, title: 'updated' };
    expect(withoutNewItems([updated, ITEM_LOW], known)).toEqual([updated]);
  });
});
