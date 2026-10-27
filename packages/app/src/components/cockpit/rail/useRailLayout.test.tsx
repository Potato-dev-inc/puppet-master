// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { RAIL_KEY, useRailLayout, type RailLayout } from './useRailLayout';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let latest: RailLayout;
function Probe() {
  latest = useRailLayout();
  return null;
}
const setWidth = (w: number) =>
  act(() => {
    Object.defineProperty(window, 'innerWidth', { value: w, configurable: true });
    window.dispatchEvent(new Event('resize'));
  });
const press = (key: string, target: EventTarget = window, init: KeyboardEventInit = {}) =>
  act(() => {
    target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }));
  });

beforeEach(() => {
  window.localStorage.clear();
  Object.defineProperty(window, 'innerWidth', { value: 1400, configurable: true });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root.render(<Probe />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('useRailLayout', () => {
  it('is open by default on a wide window', () => {
    expect(latest.visible).toBe(true);
  });
  it('hides under 1100 px whatever was chosen', () => {
    setWidth(1000);
    expect(latest.open).toBe(true);
    expect(latest.fits).toBe(false);
    expect(latest.visible).toBe(false);
    setWidth(1200);
    expect(latest.visible).toBe(true);
  });
  it('toggles with the ] key and remembers it', () => {
    press(']');
    expect(latest.open).toBe(false);
    expect(window.localStorage.getItem(RAIL_KEY)).toBe('0');
    press(']');
    expect(latest.open).toBe(true);
    expect(window.localStorage.getItem(RAIL_KEY)).toBe('1');
  });
  it('ignores ] while typing, with a modifier, or on a narrow window', () => {
    const input = document.createElement('input');
    document.body.appendChild(input);
    press(']', input);
    expect(latest.open).toBe(true);
    press(']', window, { ctrlKey: true });
    expect(latest.open).toBe(true);
    setWidth(900);
    press(']');
    expect(latest.open).toBe(true);
    input.remove();
  });
  it('starts closed when it was left closed', () => {
    window.localStorage.setItem(RAIL_KEY, '0');
    act(() => root.render(<div />));
    act(() => root.render(<Probe />));
    expect(latest.open).toBe(false);
  });
  it('still works when storage is blocked', () => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {
      throw new Error('blocked');
    };
    try {
      press(']');
      expect(latest.open).toBe(false);
    } finally {
      Storage.prototype.setItem = original;
    }
  });
});
