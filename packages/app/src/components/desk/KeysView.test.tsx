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
  center: 'ctrl+alt+c',
  lock_pointer: 'ctrl+alt+l',
  jump_n: 'ctrl+alt+shift+{n}',
  return_home: 'ctrl+alt+shift+h',
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
    { action: 'center', keys: 'ctrl+alt+c' },
    { action: 'lock', keys: 'ctrl+alt+l' },
    { action: 'jump:1', keys: 'ctrl+alt+shift+1' },
    { action: 'return_home', keys: 'ctrl+alt+shift+h' },
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
    expect(conflictMessage('screen', 'ctrl+alt+shift+1', HOTKEYS.bindings)).toBe('Ctrl Alt Shift 1 is already used by Jump to computer 1.');
    expect(conflictMessage('jump', 'ctrl+alt+shift+2', HOTKEYS.bindings)).toBeNull();
  });
});

describe('toTemplate', () => {
  it('stores the screen row with {n} in place of the digit', () => {
    expect(toTemplate('ctrl+alt+5')).toBe('ctrl+alt+{n}');
  });
});

describe('KeysView', () => {
  it('lists the nine shortcuts with the numbered rows shown as 1…9', () => {
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={vi.fn(async () => true)} />));
    for (const label of [
      'Next screen',
      'Previous screen',
      'Go to screen N',
      'Find my pointer',
      'Pause sharing',
      'Center the pointer',
      'Lock pointer to this screen',
      'Jump to computer N',
      'Bring the pointer back',
    ]) {
      expect(host.textContent).toContain(label);
    }
    expect(host.querySelectorAll('li.ck-desk-row')).toHaveLength(9);
    expect(host.textContent).toContain('Ctrl Alt 1…9');
    expect(host.textContent).toContain('Ctrl Alt Shift 1…9');
    expect(host.textContent).toContain('Scroll Lock');
  });

  it.each([
    ['Center the pointer', 'Ctrl Alt C', { key: 'k', code: 'KeyK' }, 'center', 'ctrl+alt+k'],
    ['Lock pointer to this screen', 'Ctrl Alt L', { key: 'k', code: 'KeyK' }, 'lock_pointer', 'ctrl+alt+k'],
    ['Bring the pointer back', 'Ctrl Alt Shift H', { key: 'K', code: 'KeyK', shiftKey: true }, 'return_home', 'ctrl+alt+shift+k'],
  ])('recording a key for "%s" sends set_hotkeys with that field', (label, now, press, field, keys) => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>(`[aria-label="Change ${label}, now ${now}"]`)!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { ctrlKey: true, altKey: true, ...press }));
    });
    expect(run).toHaveBeenCalledWith({ type: 'set_hotkeys', hotkeys: { ...DEFAULTS, [field]: keys } });
  });

  it('recording a computer number (Shift held, so the browser says "&") stores it as {n}', () => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Change Jump to computer N, now Ctrl Alt Shift 1…9"]')!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: '&', code: 'Digit7', ctrlKey: true, altKey: true, shiftKey: true }));
    });
    expect(run).toHaveBeenCalledWith({ type: 'set_hotkeys', hotkeys: { ...DEFAULTS, jump_n: 'ctrl+alt+shift+{n}' } });
  });

  it('a computer number needs a digit', () => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Change Jump to computer N, now Ctrl Alt Shift 1…9"]')!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', code: 'KeyJ', ctrlKey: true, altKey: true, shiftKey: true }));
    });
    expect(run).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Press a number from 1 to 9 with the same keys.');
  });

  it('the new rows name the shortcut they clash with', () => {
    const run = vi.fn(async () => true);
    act(() => root.render(<KeysView hotkeys={HOTKEYS} run={run} />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Change Center the pointer, now Ctrl Alt C"]')!.click());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', code: 'KeyL', ctrlKey: true, altKey: true }));
    });
    expect(run).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Ctrl Alt L is already used by Lock pointer to this screen.');
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
