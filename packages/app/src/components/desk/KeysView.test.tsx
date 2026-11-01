// @vitest-environment jsdom
// The hop keys list: conflicts are named from the engine's bindings, a screen row stores {n}, and Reset to defaults
// is off until a key differs from its default.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KeysView, conflictMessage, toTemplate } from './KeysView';
import type { DeskHotkeys } from '../../lib/desk';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const DEFAULTS = {
  next: 'ctrl+alt+right',
  prev: 'ctrl+alt+left',
  screen_n: 'ctrl+alt+{n}',
  find_pointer: 'ctrl+alt+f',
  pause: 'scrolllock',
};
const HOTKEYS: DeskHotkeys = {
  config: DEFAULTS,
  defaults: DEFAULTS,
  bindings: [
    { action: 'next', keys: 'ctrl+alt+right' },
    { action: 'prev', keys: 'ctrl+alt+left' },
    { action: 'screen:1', keys: 'ctrl+alt+1' },
    { action: 'find', keys: 'ctrl+alt+f' },
    { action: 'pause', keys: 'scrolllock' },
  ],
};

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

describe('conflictMessage', () => {
  it('names the shortcut that already uses the keys', () => {
    expect(conflictMessage('next', 'ctrl+alt+f', HOTKEYS.bindings)).toBe('Ctrl Alt F is already used by Find my pointer.');
  });

  it('a row does not clash with its own binding', () => {
    expect(conflictMessage('next', 'ctrl+alt+right', HOTKEYS.bindings)).toBeNull();
  });

  it('the screen row clashes with another screen number only', () => {
    expect(conflictMessage('screen', 'ctrl+alt+1', HOTKEYS.bindings)).toBeNull();
    expect(conflictMessage('screen', 'ctrl+alt+left', HOTKEYS.bindings)).toBe('Ctrl Alt Left is already used by Previous screen.');
  });
});

describe('toTemplate', () => {
  it('stores the screen row with {n} in place of the digit', () => {
    expect(toTemplate('ctrl+alt+5')).toBe('ctrl+alt+{n}');
  });
});

describe('KeysView', () => {
  it('lists the five shortcuts with the screen row shown as 1…9', () => {
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={vi.fn(async () => true)} />));
    for (const label of ['Next screen', 'Previous screen', 'Go to screen N', 'Find my pointer', 'Pause sharing']) {
      expect(host.textContent).toContain(label);
    }
    expect(host.textContent).toContain('Ctrl Alt 1…9');
    expect(host.textContent).toContain('Scroll Lock');
  });

  it('Reset to defaults is off while the keys are the defaults', () => {
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={vi.fn(async () => true)} />));
    expect(host.querySelector<HTMLButtonElement>('button.ck-desk-btn')!.disabled).toBe(true);
  });

  it('Reset to defaults sends reset_hotkeys once a key differs', () => {
    const run = vi.fn(async () => true);
    const changed = { ...HOTKEYS, config: { ...DEFAULTS, next: 'ctrl+alt+n' } };
    act(() => root.render(<KeysView hotkeys={changed} run={run} />));
    act(() => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === 'Reset to defaults')!.click());
    expect(run).toHaveBeenCalledWith({ type: 'reset_hotkeys' });
  });

  it('recording a new key sends set_hotkeys with the whole config', () => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Change Next screen, now Ctrl Alt Right"]')!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, altKey: true }));
    });
    expect(run).toHaveBeenCalledWith({ type: 'set_hotkeys', hotkeys: { ...DEFAULTS, next: 'ctrl+alt+n' } });
  });

  it('recording a screen number stores it as {n}', () => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Change Go to screen N, now Ctrl Alt 1…9"]')!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '7', ctrlKey: true, altKey: true }));
    });
    expect(run).toHaveBeenCalledWith({ type: 'set_hotkeys', hotkeys: { ...DEFAULTS, screen_n: 'ctrl+alt+{n}' } });
  });
});
