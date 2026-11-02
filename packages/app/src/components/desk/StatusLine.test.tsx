// @vitest-environment jsdom
// The one status line: plain words for every state, and one fix button only when there is an error.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StatusLine, statusText } from './StatusLine';
import type { DeskStatus } from '../../lib/desk';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const base: DeskStatus = { state: 'on', hops_enabled: true, paused: false, hooks_installed: true, pointer_locked: false, hook_reinstalls: 0, error: null };

describe('statusText', () => {
  it('says what is happening, in plain words', () => {
    expect(statusText({ ...base, hops_enabled: false, state: 'off' }, 2)).toBe('Screen hops are off');
    expect(statusText(base, 3)).toBe('On · 3 screens');
    expect(statusText(base, 1)).toBe('On · 1 screen');
    expect(statusText({ ...base, pointer_locked: true }, 2)).toBe('On · 2 screens · Pointer locked to this screen');
    expect(statusText({ ...base, paused: true, state: 'paused' }, 2)).toBe('Paused');
    expect(statusText({ ...base, state: 'unsupported' }, 0)).toBe("Screen hops don't work on this computer yet.");
  });

  it('shows an error with its words', () => {
    expect(statusText({ ...base, error: 'the keyboard hook was refused' }, 2)).toBe(
      'Screen hops hit a problem: the keyboard hook was refused',
    );
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

describe('StatusLine', () => {
  it('has no fix button while things are fine', () => {
    act(() => root.render(<StatusLine status={base} screenCount={2} onFix={vi.fn()} />));
    expect(host.querySelector('button')).toBeNull();
    expect(host.querySelector('[role="status"]')?.textContent).toBe('On · 2 screens');
  });

  it('an error shows one Try again button that calls onFix', () => {
    const onFix = vi.fn();
    act(() => root.render(<StatusLine status={{ ...base, error: 'boom' }} screenCount={2} onFix={onFix} />));
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    act(() => host.querySelector<HTMLButtonElement>('button')!.click());
    expect(onFix).toHaveBeenCalledTimes(1);
  });
});
