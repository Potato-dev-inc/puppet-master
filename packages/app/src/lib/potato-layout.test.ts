// @vitest-environment jsdom
// The page's layout rules: limits, snapping to the strip, the panel leaving the thread its room, and what is remembered
// (also when storage is blocked or holds rubbish).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clampListWidth,
  clampPanelWidth,
  LAYOUT_KEYS,
  LIST_DEFAULT,
  LIST_MINI_WIDTH,
  listFromDrag,
  listShownWidth,
  PANEL_DEFAULT,
  readLayout,
  writeLayout,
} from './potato-layout';

beforeEach(() => window.localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe('limits', () => {
  it('keeps the list between 240 and 480 px', () => {
    expect(clampListWidth(100)).toBe(240);
    expect(clampListWidth(300.4)).toBe(300);
    expect(clampListWidth(900)).toBe(480);
    expect(clampListWidth(Number.NaN)).toBe(LIST_DEFAULT);
  });

  it('keeps the panel between 320 and 1000 px', () => {
    expect(clampPanelWidth(10)).toBe(320);
    expect(clampPanelWidth(500)).toBe(500);
    expect(clampPanelWidth(5000)).toBe(1000);
    expect(clampPanelWidth(Number.NaN)).toBe(PANEL_DEFAULT);
  });

  it('leaves the thread 320 px when the room is known, but never goes under 320 for the panel', () => {
    // 1200 wide, 240 list: the panel may take 1200 - 240 - 320 = 640
    expect(clampPanelWidth(900, 1200, 240)).toBe(640);
    expect(clampPanelWidth(500, 1200, 240)).toBe(500);
    // a very narrow window: the panel keeps its minimum
    expect(clampPanelWidth(900, 700, 240)).toBe(320);
  });

  it('turns a drag below 160 px into the strip, and anything else into a clamped width', () => {
    expect(listFromDrag(120)).toEqual({ mini: true });
    expect(listFromDrag(159)).toEqual({ mini: true });
    expect(listFromDrag(160)).toEqual({ mini: false, width: 240 });
    expect(listFromDrag(350)).toEqual({ mini: false, width: 350 });
    expect(listFromDrag(2000)).toEqual({ mini: false, width: 480 });
  });

  it('shows the strip as 56 px', () => {
    expect(listShownWidth({ listWidth: 300, listMini: true })).toBe(LIST_MINI_WIDTH);
    expect(listShownWidth({ listWidth: 300, listMini: false })).toBe(300);
  });
});

describe('what is remembered', () => {
  it('starts from the defaults: list 240, panel 384 and closed', () => {
    expect(readLayout()).toEqual({ listWidth: 240, listMini: false, panelWidth: 384, panelOpen: false });
  });

  it('writes and reads back every part', () => {
    writeLayout({ listWidth: 300, listMini: true, panelWidth: 600, panelOpen: true });
    expect(window.localStorage.getItem(LAYOUT_KEYS.listWidth)).toBe('300');
    expect(window.localStorage.getItem(LAYOUT_KEYS.panelOpen)).toBe('1');
    expect(readLayout()).toEqual({ listWidth: 300, listMini: true, panelWidth: 600, panelOpen: true });
  });

  it('puts remembered widths back inside the limits, and ignores rubbish', () => {
    window.localStorage.setItem(LAYOUT_KEYS.listWidth, '9999');
    window.localStorage.setItem(LAYOUT_KEYS.panelWidth, 'wide');
    window.localStorage.setItem(LAYOUT_KEYS.panelOpen, 'maybe');
    expect(readLayout()).toEqual({ listWidth: 480, listMini: false, panelWidth: 384, panelOpen: false });
  });

  it('survives a blocked store', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readLayout().listWidth).toBe(240);
    expect(() => writeLayout({ listWidth: 300, listMini: false, panelWidth: 400, panelOpen: true })).not.toThrow();
  });
});
