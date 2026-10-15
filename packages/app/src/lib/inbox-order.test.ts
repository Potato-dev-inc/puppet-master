import { describe, expect, it } from 'vitest';
import { inboxRank, sortInbox, type InboxEntry } from './inbox-order';

const e = (key: string, kind: InboxEntry['kind'], since?: number, risk?: InboxEntry['risk']): InboxEntry => ({ key, kind, since, risk });

describe('inbox order (the Rust rank_of rule)', () => {
  it('ranks high risk and consent first, then calmer approvals and questions, then pauses, then errors', () => {
    expect(inboxRank('consent')).toBe(0);
    expect(inboxRank('approval', 'high')).toBe(0);
    expect(inboxRank('approval', 'medium')).toBe(1);
    expect(inboxRank('approval', undefined)).toBe(1);
    expect(inboxRank('approval', 'low')).toBe(2);
    expect(inboxRank('question')).toBe(2);
    expect(inboxRank('budget')).toBe(3);
    expect(inboxRank('limit')).toBe(3);
    expect(inboxRank('error')).toBe(4);
    expect(inboxRank('stuck')).toBe(4);
  });

  it('sorts by rank, then oldest first, then key', () => {
    const sorted = sortInbox([
      e('error', 'error', 10),
      e('q-new', 'question', 900),
      e('low-old', 'approval', 100, 'low'),
      e('high-new', 'approval', 800, 'high'),
      e('consent', 'consent', 500),
      e('budget', 'budget', 1),
    ]);
    expect(sorted.map((x) => x.key)).toEqual(['consent', 'high-new', 'low-old', 'q-new', 'budget', 'error']);
  });

  it('puts an unknown age last within its rank and breaks ties by key so the order never flickers', () => {
    expect(sortInbox([e('b', 'stuck'), e('a', 'stuck'), e('c', 'error', 5)]).map((x) => x.key)).toEqual(['c', 'a', 'b']);
  });

  it('does not change its input', () => {
    const input = [e('x', 'error', 1), e('y', 'consent', 2)];
    sortInbox(input);
    expect(input.map((x) => x.key)).toEqual(['x', 'y']);
  });
});
