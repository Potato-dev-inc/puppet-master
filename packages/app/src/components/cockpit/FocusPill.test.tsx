// @vitest-environment jsdom
// The sidebar Focus pill (SPEC n33 3.6 (2)): hidden while off, "Focus · N min left" and End while on, the menu
// actions, the countdown, and the `focus://changed` event. `invoke` and `listen` are mocked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FocusView } from '../../lib/focus';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import { FocusPill } from './FocusPill';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const MIN = 60_000;
const NOW = 9_000_000;

const OFF: FocusView = {
  on: false,
  sessionId: null,
  startedAtMs: null,
  endsAtMs: null,
  minutes: 0,
  extended: 0,
  held: { run_done: 0, run_failed: 0, companion: 0, review: 0, night: 0 },
  heldTotal: 0,
  waitingQuiet: 0,
  waitingUrgent: 0,
  settings: { default_minutes: 45, quiet_blocking: true },
  lastSummary: null,
};
const ON: FocusView = { ...OFF, on: true, sessionId: 'fc-1', startedAtMs: NOW, endsAtMs: NOW + 45 * MIN, minutes: 45 };

let host: HTMLDivElement;
let root: Root;
let focusView: FocusView;
const notify = vi.fn();

/** Answers the focus commands from `focusView`; `failing` names a command that rejects. */
function answer(failing?: string) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === failing) throw new Error('refused');
    if (cmd === 'focus_extend') focusView = { ...ON, endsAtMs: NOW + 60 * MIN, minutes: 60 };
    if (cmd === 'focus_end') focusView = OFF;
    return focusView;
  });
}

const pill = () => host.querySelector<HTMLElement>('[data-testid="focus-pill"]');
const text = () => host.querySelector('.ck-focus-text')?.textContent ?? '';
const menuItems = () => [...host.querySelectorAll<HTMLElement>('[role="menuitem"]')].map((b) => b.textContent);
const endButton = () => host.querySelector<HTMLButtonElement>('.ck-focus-end');
const mainButton = () => host.querySelector<HTMLButtonElement>('.ck-focus-main');

async function render() {
  await act(async () => root.render(<FocusPill notify={notify} />));
  await act(async () => {});
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] });
  vi.setSystemTime(NOW);
  focusView = OFF;
  answer();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  invoke.mockReset();
  listen.mockReset();
  notify.mockReset();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.useRealTimers();
});

describe('Focus pill while off', () => {
  it('shows nothing', async () => {
    await render();
    expect(pill()).toBeNull();
    expect(host.innerHTML).toBe('');
  });
});

describe('Focus pill while on', () => {
  beforeEach(() => {
    focusView = ON;
    answer();
  });

  it('reads "Focus · 45 min left" and has an End button', async () => {
    await render();
    expect(pill()).not.toBeNull();
    expect(text()).toBe('Focus · 45 min left');
    expect(endButton()?.textContent).toBe('End');
  });

  it('counts down: the time left is re-read every 30 seconds', async () => {
    await render();
    // The fake clock moves with the timers: 15 minutes minus 30 seconds, then the 30-second tick lands on 15 minutes.
    await act(async () => {
      vi.setSystemTime(NOW + 15 * MIN - 30_000);
      vi.advanceTimersByTime(30_000);
    });
    expect(text()).toBe('Focus · 30 min left');
  });

  it('opens a small menu with +15 min and End Focus', async () => {
    await render();
    expect(menuItems()).toEqual([]);
    await act(async () => mainButton()!.click());
    expect(menuItems()).toEqual(['+15 min', 'End Focus']);
  });

  it('+15 min calls focus_extend, shows the new time and closes the menu', async () => {
    await render();
    await act(async () => mainButton()!.click());
    await act(async () => host.querySelector<HTMLButtonElement>('[role="menuitem"]')!.click());
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('focus_extend');
    expect(text()).toBe('Focus · 60 min left');
    expect(menuItems()).toEqual([]);
  });

  it('End Focus in the menu calls focus_end and the pill goes away', async () => {
    await render();
    await act(async () => mainButton()!.click());
    await act(async () => host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')[1].click());
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('focus_end');
    expect(pill()).toBeNull();
  });

  it('the End button ends Focus at once, with no menu', async () => {
    await render();
    await act(async () => endButton()!.click());
    await act(async () => {});
    expect(invoke).toHaveBeenCalledWith('focus_end');
    expect(pill()).toBeNull();
    expect(menuItems()).toEqual([]);
  });

  it('closes the menu on Escape', async () => {
    await render();
    await act(async () => mainButton()!.click());
    expect(menuItems()).toEqual(['+15 min', 'End Focus']);
    await act(async () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(menuItems()).toEqual([]);
  });

  it('says one plain line when End fails, and keeps the pill', async () => {
    answer('focus_end');
    await render();
    await act(async () => endButton()!.click());
    await act(async () => {});
    expect(notify).toHaveBeenCalledWith("Couldn't end Focus. Try again.");
    expect(pill()).not.toBeNull();
  });

  it('says one plain line when +15 min fails', async () => {
    answer('focus_extend');
    await render();
    await act(async () => mainButton()!.click());
    await act(async () => host.querySelector<HTMLButtonElement>('[role="menuitem"]')!.click());
    await act(async () => {});
    expect(notify).toHaveBeenCalledWith("Couldn't add time to Focus. Try again.");
  });
});

describe('Focus pill and the focus://changed event', () => {
  it('appears when the event says Focus is on, and goes away when it says off', async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    let handler: ((e: { payload: FocusView }) => void) | undefined;
    listen.mockImplementation(async (_name: string, cb: (e: { payload: FocusView }) => void) => {
      handler = cb;
      return () => {};
    });
    await render();
    expect(pill()).toBeNull();
    await act(async () => handler!({ payload: ON }));
    expect(text()).toBe('Focus · 45 min left');
    await act(async () => handler!({ payload: OFF }));
    expect(pill()).toBeNull();
  });
});
