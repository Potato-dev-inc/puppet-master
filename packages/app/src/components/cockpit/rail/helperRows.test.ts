import { describe, expect, it } from 'vitest';
import { helperRows } from './helperRows';

const pane = (id: string, parent?: string, title?: string, status = 'idle') => ({ status, info: { id, title, parent_pane_id: parent } });

describe('helperRows', () => {
  it('lists only the direct helpers of the pane, with a title and agent', () => {
    const list = [pane('p'), pane('a', 'p', 'Check tests', 'running'), pane('b', 'p'), pane('c', 'a'), pane('other')];
    expect(helperRows(list, 'p', () => 'Codex')).toEqual([
      { id: 'a', title: 'Check tests', agent: 'Codex', status: 'running' },
      { id: 'b', title: 'Codex', agent: null, status: 'idle' },
    ]);
  });
  it('is empty for a pane without helpers', () => {
    expect(helperRows([pane('p')], 'p', () => 'x')).toEqual([]);
  });
});
