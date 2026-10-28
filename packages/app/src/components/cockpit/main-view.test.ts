import { describe, expect, it } from 'vitest';
import {
  clampSplitRatio,
  homeView,
  isSplitShortcut,
  nudgeSplitRatio,
  ratioFromLeftPx,
  splitAfterSelect,
  splitFits,
  splitFocusKey,
  splitPartner,
  splitStillOn,
  viewAfterSelect,
  SPLIT_SIDE_MIN_PX,
  type MainView,
} from './main-view';

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

describe('split view: store', () => {
  it('opens with the pane above the selected one, else the one below', () => {
    expect(splitPartner(['a', 'b', 'c'], 'b')).toBe('a');
    expect(splitPartner(['a', 'b', 'c'], 'c')).toBe('b');
    expect(splitPartner(['a', 'b', 'c'], 'a')).toBe('b');
    expect(splitPartner(['a'], 'a')).toBeNull();
    expect(splitPartner(['a', 'b'], null)).toBeNull();
    expect(splitPartner(['a', 'b'], 'gone')).toBeNull();
  });

  it('a pick keeps the split, the inbox ends it, and picking the right pane swaps the sides', () => {
    expect(splitAfterSelect('b', 'a', 'c')).toBe('b');
    expect(splitAfterSelect('b', 'a', null)).toBeNull();
    expect(splitAfterSelect('b', 'a', 'b')).toBe('a');
    expect(splitAfterSelect(null, 'a', 'b')).toBeNull();
  });

  it('stays on only when the window is wide, a pane is selected, and the right pane still exists', () => {
    const has = (ids: string[]) => (id: string) => ids.includes(id);
    const ok = { selectedId: 'a', width: 1200, hasPane: has(['a', 'b']) };
    expect(splitStillOn('b', ok)).toBe(true);
    expect(splitStillOn('b', { ...ok, width: 899 })).toBe(false);
    expect(splitStillOn('b', { ...ok, width: 900 })).toBe(true);
    expect(splitStillOn('b', { ...ok, selectedId: null })).toBe(false);
    expect(splitStillOn('b', { ...ok, hasPane: has(['a']) })).toBe(false);
    expect(splitStillOn('a', ok)).toBe(false);
    expect(splitStillOn(null, ok)).toBe(false);
  });

  it('auto-off below 900px', () => {
    expect(splitFits(899)).toBe(false);
    expect(splitFits(900)).toBe(true);
  });
});

describe('split view: Ctrl+\\ shortcut', () => {
  const press = (over: Partial<KeyboardEvent>) => ({ key: '\\', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, ...over });

  it('is Ctrl and backslash with no other modifier', () => {
    expect(isSplitShortcut(press({}))).toBe(true);
    expect(isSplitShortcut(press({ ctrlKey: false }))).toBe(false);
    expect(isSplitShortcut(press({ key: 'k' }))).toBe(false);
    expect(isSplitShortcut(press({ shiftKey: true }))).toBe(false);
    expect(isSplitShortcut(press({ altKey: true }))).toBe(false);
    expect(isSplitShortcut(press({ metaKey: true }))).toBe(false);
  });
});

describe('split view: resize', () => {
  // A 1000px split: the divider takes 9px, so the two sides share 991px.
  const SPAN = 991;

  it('keeps each side at least 320px wide', () => {
    expect(clampSplitRatio(0.05, SPAN)).toBeCloseTo(SPLIT_SIDE_MIN_PX / SPAN);
    expect(clampSplitRatio(0.95, SPAN)).toBeCloseTo(1 - SPLIT_SIDE_MIN_PX / SPAN);
    expect(clampSplitRatio(0.4, SPAN)).toBe(0.4);
  });

  it('falls back to the even split when there is no room for two minimum sides or the ratio is not a number', () => {
    expect(clampSplitRatio(0.9, 600)).toBe(0.5);
    expect(clampSplitRatio(Number.NaN, SPAN)).toBe(0.5);
  });

  it('an arrow press moves the divider 24px either way, and stops at the minimum', () => {
    expect(nudgeSplitRatio(0.5, 24, SPAN)).toBeCloseTo(0.5 + 24 / SPAN);
    expect(nudgeSplitRatio(0.5, -24, SPAN)).toBeCloseTo(0.5 - 24 / SPAN);
    expect(nudgeSplitRatio(0.33, -24, SPAN)).toBeCloseTo(SPLIT_SIDE_MIN_PX / SPAN);
    expect(nudgeSplitRatio(0.5, 24, 0)).toBe(0.5);
  });

  it('a dragged divider maps its left edge to a clamped ratio', () => {
    expect(ratioFromLeftPx(495.5, SPAN)).toBeCloseTo(0.5);
    expect(ratioFromLeftPx(10, SPAN)).toBeCloseTo(SPLIT_SIDE_MIN_PX / SPAN);
    expect(ratioFromLeftPx(900, SPAN)).toBeCloseTo(1 - SPLIT_SIDE_MIN_PX / SPAN);
    expect(ratioFromLeftPx(100, 0)).toBe(0.5);
  });
});

describe('split view: focus keys', () => {
  const key = (over: Partial<KeyboardEvent>) => ({ key: 'ArrowLeft', ctrlKey: true, metaKey: false, altKey: true, shiftKey: false, ...over });

  it('on Windows, Ctrl+Alt+Left and Ctrl+Alt+Right pick the side', () => {
    expect(splitFocusKey(key({}), false)).toBe('left');
    expect(splitFocusKey(key({ key: 'ArrowRight' }), false)).toBe('right');
  });

  it('on a Mac, Cmd+Alt is the key and Ctrl is not', () => {
    expect(splitFocusKey(key({ ctrlKey: false, metaKey: true }), true)).toBe('left');
    expect(splitFocusKey(key({ ctrlKey: false, metaKey: true, key: 'ArrowRight' }), true)).toBe('right');
    expect(splitFocusKey(key({ ctrlKey: true, metaKey: false }), true)).toBeNull();
  });

  it('any other modifier, key or missing Alt is not ours', () => {
    expect(splitFocusKey(key({ altKey: false }), false)).toBeNull();
    expect(splitFocusKey(key({ shiftKey: true }), false)).toBeNull();
    expect(splitFocusKey(key({ key: 'ArrowUp' }), false)).toBeNull();
    expect(splitFocusKey(key({ key: 'a' }), false)).toBeNull();
    expect(splitFocusKey(key({ ctrlKey: false }), false)).toBeNull();
  });
});
