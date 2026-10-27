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
