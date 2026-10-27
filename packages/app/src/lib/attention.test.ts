import { describe, expect, it } from 'vitest';
import { attentionMismatch, blockingItems, deriveNeedsYou, isBlockingKind, itemsOfKind, needsYouPaneIds, type AttentionSnapshot } from './attention';

const panes = [
  { id: 'a', status: 'waiting_input' },
  { id: 'b', status: 'waiting_input' },
  { id: 'c', status: 'error' },
  { id: 'd', status: 'running' },
  { id: 'e', status: 'idle' },
];

describe('deriveNeedsYou', () => {
  it('is empty when nothing needs the person', () => {
    expect(deriveNeedsYou({ panes: [{ id: 'x', status: 'running' }, { id: 'y', status: 'idle' }] })).toEqual([]);
  });

  it('has one item per approval, question, error, stuck pane and Ask-bar answer', () => {
    const items = deriveNeedsYou({
      panes,
      prompts: { a: { risk: { level: 'high' } }, b: { risk: { level: 'low' } } },
      questions: [{ id: 'q1', pane_id: 'd', created_at_ms: 5 }, { id: 'q2' }],
      stuckPaneIds: ['d'],
      notices: [{ id: 'n1', at_ms: 9 }],
      statusSince: new Map([['a', 100]]),
    });
    expect(items.map((i) => i.id)).toEqual([
      'approval:a',
      'approval:b',
      'question:q1',
      'question:q2',
      'error:c',
      'stuck:d',
      'answer:n1',
    ]);
    expect(items[0]).toMatchObject({ kind: 'approval', paneId: 'a', risk: 'high', since: 100 });
    expect(items[1].risk).toBe('low');
    expect(items[3].paneId).toBeUndefined();
    expect(itemsOfKind(items, 'error')).toHaveLength(1);
  });

  it('counts an errored pane (the old Inbox count missed it)', () => {
    const items = deriveNeedsYou({ panes: [{ id: 'ps', status: 'error' }] });
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe('error');
  });

  it('ignores stuck signals and question panes for panes that no longer exist', () => {
    const items = deriveNeedsYou({ panes: [], stuckPaneIds: ['gone'], questions: [{ id: 'q', pane_id: 'gone' }] });
    expect(items.map((i) => i.id)).toEqual(['question:q']);
    expect(items[0].paneId).toBeUndefined();
  });

  it('drops a risk level it does not know', () => {
    const [item] = deriveNeedsYou({ panes: [{ id: 'a', status: 'waiting_input' }], prompts: { a: { risk: { level: 'spicy' } } } });
    expect(item.risk).toBeUndefined();
  });

  it('lists the panes that have something waiting, for the sidebar group', () => {
    const items = deriveNeedsYou({ panes, stuckPaneIds: ['d'], notices: [{ id: 'n' }] });
    expect([...needsYouPaneIds(items)].sort()).toEqual(['a', 'b', 'c', 'd']);
  });
});

describe('companions in the one list', () => {
  it('only a companion that stopped for good is an item, keyed like the Rust card', () => {
    const items = deriveNeedsYou({
      panes: [],
      companions: [
        { id: 'mochi', state: 'stopped' },
        { id: 'bit', state: 'needs_you' },
        { id: 'nib', state: 'idle' },
      ],
    });
    expect(items.map((i) => i.id)).toEqual(['companion:mochi']);
    expect(blockingItems(items)).toHaveLength(1);
  });
});

describe('missions in the one list', () => {
  it('a mission that needs you is one blocking item keyed like the Rust card; its budget card is a question already', () => {
    const items = deriveNeedsYou({
      panes: [],
      missions: [
        { id: 'm1', state: 'needs_you', needs: 'tamper' },
        { id: 'm2', state: 'needs_you', needs: 'budget' },
        { id: 'm3', state: 'working', needs: null },
        { id: 'm4', state: 'needs_you', needs: 'stuck' },
      ],
    });
    expect(items.map((i) => i.id)).toEqual(['mission:m1', 'mission:m4']);
    expect(blockingItems(items)).toHaveLength(2);
  });

  it('agrees with the Rust snapshot, so the desktop number, the tray and the phone count a mission card once', () => {
    const items = deriveNeedsYou({ panes: [], missions: [{ id: 'm1', state: 'needs_you', needs: 'setup' }] });
    const snapshot = {
      v: 1,
      rev: 1,
      counts: { blocking: 1, ready: 0 },
      items: [{ id: 'x', key: 'mission:m1', kind: 'setup', lane: 'blocking', state: 'open' }],
    } as unknown as AttentionSnapshot;
    expect(attentionMismatch(items, snapshot, isBlockingKind)).toEqual([]);
  });
});
