import { describe, expect, it } from 'vitest';
import { ancestorIds, bubbleTargets, helperAsksLabel, helperItems, helpersAskingFor } from './helper-questions';

const pane = (id: string, parent?: string, status = 'running') => ({ info: { id, parent_pane_id: parent }, status });
// root > mid > leaf, and root > side; other is unrelated.
const list = [pane('root'), pane('mid', 'root'), pane('leaf', 'mid'), pane('side', 'root'), pane('other')];

describe('bubbleTargets', () => {
  it('goes to the direct parent and the top-most ancestor', () => {
    expect(bubbleTargets(list, 'leaf')).toEqual(['mid', 'root']);
  });
  it('counts one pane once when the parent is the top', () => {
    expect(bubbleTargets(list, 'mid')).toEqual(['root']);
    expect(bubbleTargets(list, 'side')).toEqual(['root']);
  });
  it('has no target for a pane nobody started, or whose parent is gone', () => {
    expect(bubbleTargets(list, 'other')).toEqual([]);
    expect(bubbleTargets([pane('orphan', 'closed')], 'orphan')).toEqual([]);
  });
  it('survives a malformed loop', () => {
    const loop = [pane('a', 'b'), pane('b', 'a')];
    expect(ancestorIds(loop, 'a')).toEqual([]);
  });
});

describe('helpersAskingFor', () => {
  const asking = (ids: string[]) => (id: string) => ids.includes(id);
  it('lists the helpers that ask, for the parent and for the top-most ancestor', () => {
    const q = asking(['leaf', 'side']);
    expect(helpersAskingFor(list, 'mid', q)).toEqual(['leaf']);
    expect(helpersAskingFor(list, 'root', q)).toEqual(['leaf', 'side']);
  });
  it('shows nothing in a pane that is not a target, nor in the helper itself', () => {
    const q = asking(['leaf']);
    expect(helpersAskingFor(list, 'side', q)).toEqual([]);
    expect(helpersAskingFor(list, 'leaf', q)).toEqual([]);
    expect(helpersAskingFor(list, 'other', q)).toEqual([]);
  });
  it('drops a question as soon as the helper stops asking (answered or gone)', () => {
    expect(helpersAskingFor(list, 'root', asking(['leaf']))).toEqual(['leaf']);
    expect(helpersAskingFor(list, 'root', asking([]))).toEqual([]);
    expect(helpersAskingFor(list.filter((p) => p.info.id !== 'leaf'), 'root', asking(['leaf']))).toEqual([]);
  });
  it('a pane with no helpers shows nothing', () => {
    expect(helpersAskingFor([pane('solo')], 'solo', () => true)).toEqual([]);
  });
});

describe('helperAsksLabel', () => {
  it('names the helper agent', () => {
    expect(helperAsksLabel('Codex')).toBe('Codex helper asks');
  });
});

describe('helperItems', () => {
  const item = (id: string, pane: string | null, state = 'open') => ({ id, state, pane: pane ? { id: pane } : null });
  it('takes the first open card of each asking helper, in helper order', () => {
    const items = [item('a', 'side'), item('b', 'leaf'), item('c', 'leaf'), item('d', 'other')];
    expect(helperItems(items, ['leaf', 'side'], () => true).map((i) => i.id)).toEqual(['b', 'a']);
  });
  it('skips resolved items and items that are not answerable cards', () => {
    const items = [item('a', 'leaf', 'resolved'), item('b', 'leaf')];
    expect(helperItems(items, ['leaf'], (i) => i.id !== 'b')).toEqual([]);
    expect(helperItems(items, ['leaf'], () => true).map((i) => i.id)).toEqual(['b']);
  });
});

// ---- two helpers under one parent: a card is never mapped to the other helper (safety net) ----

describe('two helpers under one parent', () => {
  // parent > a, parent > b (siblings); nothing else.
  const team = [pane('parent'), pane('a', 'parent'), pane('b', 'parent')];
  type Item = { id: string; state: string; pane: { id: string } | null; prompt?: string };
  const item = (id: string, helper: string | null, state = 'open', prompt = 'npm test'): Item => ({ id, state, pane: helper ? { id: helper } : null, prompt });
  const isCard = (i: Item) => i.prompt !== undefined;

  it('shows both questions in the parent, each in its own helper slot', () => {
    expect(helpersAskingFor(team, 'parent', (id) => id === 'a' || id === 'b')).toEqual(['a', 'b']);
    // The helpers do not see each other's questions.
    expect(helpersAskingFor(team, 'a', () => true)).toEqual([]);
    expect(helpersAskingFor(team, 'b', () => true)).toEqual([]);
  });

  it('gives each helper only the card of its own pane, even when both ask the same words', () => {
    const items = [item('card-a', 'a'), item('card-b', 'b')];
    expect(helperItems(items, ['a', 'b'], isCard).map((i) => i.id)).toEqual(['card-a', 'card-b']);
    expect(helperItems(items, ['a'], isCard).map((i) => i.id)).toEqual(['card-a']);
    expect(helperItems(items, ['b'], isCard).map((i) => i.id)).toEqual(['card-b']);
  });

  it('after A moved on, A shows its new card and never the old one, and B keeps its own', () => {
    // A's first card was answered (resolved); A asks something else; B still asks the first question.
    const items = [item('card-a1', 'a', 'resolved'), item('card-a2', 'a', 'open', 'npm run build'), item('card-b1', 'b')];
    const shown = helperItems(items, ['a', 'b'], isCard);
    expect(shown.map((i) => i.id)).toEqual(['card-a2', 'card-b1']);
    expect(shown.find((i) => i.id === 'card-a1')).toBeUndefined();
    // The card an answer would go to is the one found for that helper: B never receives A's.
    expect(helperItems(items, ['b'], isCard).map((i) => i.id)).toEqual(['card-b1']);
  });

  it('shows nothing for A while it has no open card, and still shows B', () => {
    const items = [item('card-a1', 'a', 'resolved'), item('card-b1', 'b')];
    expect(helperItems(items, ['a', 'b'], isCard).map((i) => i.id)).toEqual(['card-b1']);
    expect(helperItems(items, ['a'], isCard)).toEqual([]);
  });

  it('never takes a card that belongs to no pane or to a pane that is not asking', () => {
    const items = [item('loose', null), item('card-other', 'other'), item('card-a', 'a')];
    expect(helperItems(items, ['a', 'b'], isCard).map((i) => i.id)).toEqual(['card-a']);
  });

  it('a helper that closed drops out of the parent and its card goes with it', () => {
    const gone = team.filter((p) => p.info.id !== 'a');
    const asking = (id: string) => id === 'a' || id === 'b';
    expect(helpersAskingFor(gone, 'parent', asking)).toEqual(['b']);
  });
});
