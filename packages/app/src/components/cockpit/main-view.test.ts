import { describe, expect, it } from 'vitest';
import { homeView, viewAfterSelect, type MainView } from './main-view';

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

describe('cockpit main view: companion page', () => {
  it('the companion page is a main view of its own, and picking any pane or the inbox leaves it', () => {
    const page: MainView = 'companion';
    expect(page).toBe('companion');
    expect(viewAfterSelect(null, 'p1')).not.toBe('companion');
    expect(viewAfterSelect(null, null)).not.toBe('companion');
    expect(homeView(false, null)).not.toBe('companion');
  });
});
