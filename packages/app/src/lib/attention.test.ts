import { describe, expect, it } from 'vitest';
import { deriveNeedsYou, itemsOfKind, needsYouPaneIds } from './attention';

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
