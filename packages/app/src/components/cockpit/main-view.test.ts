import { describe, expect, it } from 'vitest';
import { homeView, viewAfterSelect } from './main-view';

describe('cockpit main view', () => {
  it('rests on a starting agent, else the selected pane, else the inbox', () => {
    expect(homeView(true, 'p1')).toBe('spawn');
    expect(homeView(true, null)).toBe('spawn');
    expect(homeView(false, 'p1')).toBe('pane');
    expect(homeView(false, null)).toBe('inbox');
  });

  it('a selection keeps a running spawn in front and drops a failed one', () => {
    expect(viewAfterSelect(null, 'p1')).toBe('pane');
    expect(viewAfterSelect(null, null)).toBe('inbox');
    expect(viewAfterSelect({ failed: null }, 'p1')).toBe('spawn');
    expect(viewAfterSelect({ failed: 'x' }, 'p1')).toBe('pane');
    expect(viewAfterSelect({ failed: 'x' }, null)).toBe('spawn');
  });
});
