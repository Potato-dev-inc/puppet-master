import { describe, expect, it } from 'vitest';
import { visibleSelection } from './selection';

const listed = new Map([['a', 1]]);

describe('visibleSelection', () => {
  it('keeps a listed pane', () => {
    expect(visibleSelection('a', listed, new Set(['a']), true, false)).toBe('a');
  });
  it('leaves a pane that was listed and is gone at once (a merge closed it): no "Opening pane" flash', () => {
    expect(visibleSelection('gone', listed, new Set(['a', 'gone']), true, false)).toBeNull();
  });
  it('keeps a pane that was never listed: it is still opening', () => {
    expect(visibleSelection('new', listed, new Set(['a']), true, false)).toBe('new');
  });
  it('keeps everything until the first list has loaded, and while a spawn is running', () => {
    expect(visibleSelection('gone', new Map(), new Set(['gone']), false, false)).toBe('gone');
    expect(visibleSelection('gone', listed, new Set(['gone']), true, true)).toBe('gone');
  });
  it('has nothing to show without a selection', () => {
    expect(visibleSelection(null, listed, new Set(), true, false)).toBeNull();
  });
});
