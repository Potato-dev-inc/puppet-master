// @vitest-environment jsdom
// Ctrl/Cmd+B and Ctrl/Cmd+Shift+B (Ctrl+\ is the split view's, so the page does not take it).
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import { isListShortcut, isPanelShortcut, usePotatoKeys } from './potato-keys';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const key = (init: KeyboardEventInit) => new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });

describe('the shortcut tests', () => {
  it('Ctrl+B or Cmd+B is the list; with Shift it is the panel', () => {
    expect(isListShortcut(key({ key: 'b', ctrlKey: true }))).toBe(true);
    expect(isListShortcut(key({ key: 'b', metaKey: true }))).toBe(true);
    expect(isListShortcut(key({ key: 'B', ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isPanelShortcut(key({ key: 'B', ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(isPanelShortcut(key({ key: 'B', metaKey: true, shiftKey: true }))).toBe(true);
    expect(isPanelShortcut(key({ key: 'b', ctrlKey: true }))).toBe(false);
  });

  it("a plain b, Alt+B and Ctrl+\\ are not the page's", () => {
    for (const e of [key({ key: 'b' }), key({ key: 'b', altKey: true, ctrlKey: true }), key({ key: '\\', ctrlKey: true })]) {
      expect(isListShortcut(e)).toBe(false);
      expect(isPanelShortcut(e)).toBe(false);
    }
  });
});

function Probe({ toggleList, togglePanel }: { toggleList: () => void; togglePanel: () => void }) {
  usePotatoKeys({ toggleList, togglePanel });
  return null;
}

describe('usePotatoKeys', () => {
  it('toggles the right thing, stops the browser, and lets go when the page goes away', () => {
    const toggleList = vi.fn();
    const togglePanel = vi.fn();
    const host = document.createElement('div');
    const root = createRoot(host);
    act(() => root.render(<Probe toggleList={toggleList} togglePanel={togglePanel} />));

    const list = key({ key: 'b', ctrlKey: true });
    act(() => void window.dispatchEvent(list));
    expect(toggleList).toHaveBeenCalledTimes(1);
    expect(list.defaultPrevented).toBe(true);

    act(() => void window.dispatchEvent(key({ key: 'B', ctrlKey: true, shiftKey: true })));
    expect(togglePanel).toHaveBeenCalledTimes(1);
    expect(toggleList).toHaveBeenCalledTimes(1);

    // typing in the message box does not get in the way: these are chords
    const field = document.createElement('textarea');
    document.body.appendChild(field);
    act(() => void field.dispatchEvent(key({ key: 'b', metaKey: true })));
    expect(toggleList).toHaveBeenCalledTimes(2);
    field.remove();

    // the split view's key is left alone
    const split = key({ key: '\\', ctrlKey: true });
    act(() => void window.dispatchEvent(split));
    expect(split.defaultPrevented).toBe(false);

    act(() => root.unmount());
    act(() => void window.dispatchEvent(key({ key: 'b', ctrlKey: true })));
    expect(toggleList).toHaveBeenCalledTimes(2);
  });
});
