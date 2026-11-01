// @vitest-environment jsdom
// The shortcut recorder: key presses become engine strings, Esc cancels, a conflict keeps the field open, and
// bare typing keys are refused so the person can still type.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KeyRecorder, chordFromEvent, formatKeys, normalizeKeys } from './KeyRecorder';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const none = { ctrlKey: false, altKey: false, shiftKey: false, metaKey: false };
const press = (key: string, mods: Partial<typeof none> = {}) => ({ key, ...none, ...mods });

describe('chordFromEvent', () => {
  it('builds the engine string from the modifiers and the key', () => {
    expect(chordFromEvent(press('k', { ctrlKey: true, altKey: true }))).toEqual({ ok: true, keys: 'ctrl+alt+k' });
    expect(chordFromEvent(press('ArrowRight', { ctrlKey: true, altKey: true }))).toEqual({ ok: true, keys: 'ctrl+alt+right' });
    expect(chordFromEvent(press('5', { ctrlKey: true, altKey: true }))).toEqual({ ok: true, keys: 'ctrl+alt+5' });
  });

  it('allows Scroll Lock and F keys with no modifier', () => {
    expect(chordFromEvent(press('ScrollLock'))).toEqual({ ok: true, keys: 'scrolllock' });
    expect(chordFromEvent(press('F5'))).toEqual({ ok: true, keys: 'f5' });
  });

  it('refuses a plain letter, so typing still works', () => {
    expect(chordFromEvent(press('a'))).toEqual({ ok: false, message: 'Add Ctrl, Alt or Shift, so typing still works.' });
  });

  it('refuses keys the engine cannot use', () => {
    expect(chordFromEvent(press('Dead', { ctrlKey: true }))).toEqual({ ok: false, message: "That key can't be used for a shortcut." });
  });
});

describe('formatKeys and normalizeKeys', () => {
  it('reads the way the person reads it', () => {
    expect(formatKeys('ctrl+alt+right')).toBe('Ctrl Alt Right');
    expect(formatKeys('ctrl+alt+{n}')).toBe('Ctrl Alt 1…9');
    expect(formatKeys('scrolllock')).toBe('Scroll Lock');
  });

  it('compares shortcuts whatever the order of their parts', () => {
    expect(normalizeKeys('alt+ctrl+k')).toBe(normalizeKeys('ctrl+alt+k'));
  });
});

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

function renderRecorder(check: (keys: string) => string | null = () => null) {
  const onRecord = vi.fn();
  act(() => root.render(<KeyRecorder label="Next screen" value="ctrl+alt+right" check={check} onRecord={onRecord} />));
  return onRecord;
}
const keydown = (init: KeyboardEventInit) => act(() => window.dispatchEvent(new KeyboardEvent('keydown', { ...init, cancelable: true })));
const openField = () => act(() => host.querySelector<HTMLButtonElement>('button.ck-desk-keys')!.click());

describe('KeyRecorder', () => {
  it('shows the current keys and asks for new ones on click', () => {
    renderRecorder();
    expect(host.querySelector('button.ck-desk-keys')?.textContent).toBe('Ctrl Alt Right');
    openField();
    expect(host.textContent).toContain('Press the new keys… Esc to cancel.');
  });

  it('records the new keys and leaves the field', () => {
    const onRecord = renderRecorder();
    openField();
    keydown({ key: 'Control', ctrlKey: true });
    keydown({ key: 'k', ctrlKey: true, altKey: true });
    expect(onRecord).toHaveBeenCalledWith('ctrl+alt+k');
    expect(host.querySelector('button.ck-desk-keys')).not.toBeNull();
  });

  it('Esc cancels and records nothing', () => {
    const onRecord = renderRecorder();
    openField();
    keydown({ key: 'Escape' });
    expect(onRecord).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain('Press the new keys');
  });

  it('a conflict shows its message and keeps the field open', () => {
    const onRecord = renderRecorder((keys) => (keys === 'ctrl+alt+f' ? 'Ctrl Alt F is already used by Find my pointer.' : null));
    openField();
    keydown({ key: 'f', ctrlKey: true, altKey: true });
    expect(onRecord).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Ctrl Alt F is already used by Find my pointer.');
    expect(host.textContent).toContain('Press the new keys');
  });

  it('a plain letter is refused with a plain message', () => {
    const onRecord = renderRecorder();
    openField();
    keydown({ key: 'a' });
    expect(onRecord).not.toHaveBeenCalled();
    expect(host.textContent).toContain('Add Ctrl, Alt or Shift, so typing still works.');
  });
});
