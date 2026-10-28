// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SHORTCUT_GROUPS } from '../../lib/shortcuts';
import { ShortcutsSheet } from './ShortcutsSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const escape = () =>
  act(() => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  });

describe('ShortcutsSheet', () => {
  it('shows the four groups and a row per shortcut, in the ck-sheet style', () => {
    act(() => root.render(<ShortcutsSheet onClose={vi.fn()} mac={false} />));
    expect(host.querySelector('.ck-sheet.ck-sheet--shortcuts')).not.toBeNull();
    const headings = [...host.querySelectorAll('.ck-shortcuts-group h3')].map((h) => h.textContent);
    expect(headings).toEqual([...SHORTCUT_GROUPS]);
    expect(host.querySelectorAll('.ck-shortcuts-row').length).toBeGreaterThan(10);
    expect(host.textContent).toContain('Split the view with the next pane');
  });

  it('shows ⌘ instead of Ctrl on a Mac', () => {
    act(() => root.render(<ShortcutsSheet onClose={vi.fn()} mac={true} />));
    const keys = [...host.querySelectorAll('.ck-shortcuts-keys')].map((k) => k.textContent);
    expect(keys).toContain('⌘K');
    expect(keys).toContain('⌘⇧F');
    expect(keys.some((k) => k?.includes('Ctrl'))).toBe(false);
  });

  it('Esc closes it', () => {
    const onClose = vi.fn();
    act(() => root.render(<ShortcutsSheet onClose={onClose} mac={false} />));
    escape();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('the Close button closes it', () => {
    const onClose = vi.fn();
    act(() => root.render(<ShortcutsSheet onClose={onClose} mac={false} />));
    const close = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Close')!;
    act(() => close.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
