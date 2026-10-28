// @vitest-environment jsdom
// The Focus rows of the palette and their actions (SPEC n33 3.6 (3)): off, on, the last used length first, and
// what each row calls. Only `lib/focus.ts` is involved; `invoke` is mocked.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import { focusPaletteItems, focusIsOn, lastFocusMinutes, presetOrder, rememberFocusMinutes } from './palette-focus';
import type { FocusView } from '../../lib/focus';

const MIN = 60_000;
const NOW = 5_000_000;

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
const ON: FocusView = { ...OFF, on: true, sessionId: 'fc-1', startedAtMs: NOW - 14 * MIN, endsAtMs: NOW + 31 * MIN, minutes: 45 };

const titles = (rows: { title: string }[]) => rows.map((r) => r.title);

beforeEach(() => {
  window.localStorage.clear();
});
afterEach(() => {
  invoke.mockReset();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe('Focus palette rows while off', () => {
  it('offers the three lengths in the usual order, and no End row', () => {
    const rows = focusPaletteItems(OFF, NOW, null, vi.fn());
    expect(titles(rows)).toEqual(['Focus 25 minutes', 'Focus 45 minutes', 'Focus 90 minutes']);
    expect(rows.every((r) => r.group === 'Actions')).toBe(true);
    expect(rows.some((r) => r.title === 'End Focus')).toBe(false);
  });

  it('treats a view that is not read yet as off', () => {
    expect(titles(focusPaletteItems(null, NOW, null, vi.fn()))).toEqual(['Focus 25 minutes', 'Focus 45 minutes', 'Focus 90 minutes']);
  });

  it('treats a view whose end is already past as off', () => {
    expect(focusIsOn({ ...ON, endsAtMs: NOW - 1 }, NOW)).toBe(false);
    expect(titles(focusPaletteItems({ ...ON, endsAtMs: NOW - 1 }, NOW, null, vi.fn()))).toEqual([
      'Focus 25 minutes',
      'Focus 45 minutes',
      'Focus 90 minutes',
    ]);
  });
});

describe('Focus palette rows while on', () => {
  it('shows End Focus with the time left, Add 15 minutes, then the lengths again', () => {
    const rows = focusPaletteItems(ON, NOW, null, vi.fn());
    expect(titles(rows)).toEqual([
      'End Focus',
      'Add 15 minutes to Focus',
      'Focus 25 minutes',
      'Focus 45 minutes',
      'Focus 90 minutes',
    ]);
    expect(rows[0].desc).toBe('31 min left');
    expect(rows[2].desc).toBe('Starts again from now');
  });

  it('calls focus_end from End Focus and focus_extend from Add 15 minutes', async () => {
    invoke.mockResolvedValue(OFF);
    const rows = focusPaletteItems(ON, NOW, null, vi.fn());
    rows[0].run();
    rows[1].run();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    expect(invoke).toHaveBeenNthCalledWith(1, 'focus_end');
    expect(invoke).toHaveBeenNthCalledWith(2, 'focus_extend');
  });

  it('says one plain line when End Focus fails', async () => {
    invoke.mockRejectedValue(new Error('boom'));
    const notify = vi.fn();
    focusPaletteItems(ON, NOW, null, notify)[0].run();
    await vi.waitFor(() => expect(notify).toHaveBeenCalledWith("Couldn't end Focus. Try again."));
  });
});

describe('Focus palette: the last used length first', () => {
  it('puts the length started last at the front and keeps the other two in order', () => {
    expect(presetOrder(90)).toEqual([90, 25, 45]);
    expect(presetOrder(25)).toEqual([25, 45, 90]);
    expect(titles(focusPaletteItems(OFF, NOW, 90, vi.fn()))).toEqual(['Focus 90 minutes', 'Focus 25 minutes', 'Focus 45 minutes']);
  });

  it('keeps the usual order when nothing was started here, or the saved value is not a preset', () => {
    expect(presetOrder(null)).toEqual([25, 45, 90]);
    expect(presetOrder(60)).toEqual([25, 45, 90]);
  });

  it('remembers the length a row started, and a row starts it with focus_start', async () => {
    invoke.mockResolvedValue(ON);
    const rows = focusPaletteItems(OFF, NOW, null, vi.fn());
    rows[2].run(); // Focus 90 minutes
    expect(lastFocusMinutes()).toBe(90);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('focus_start', { minutes: 90 }));
  });

  it('reads nothing it cannot trust: a damaged value is ignored', () => {
    window.localStorage.setItem('pm.focus.lastMinutes', 'abc');
    expect(lastFocusMinutes()).toBeNull();
    window.localStorage.setItem('pm.focus.lastMinutes', '60');
    expect(lastFocusMinutes()).toBeNull();
  });

  it('works without storage (blocked): the usual order and no crash', () => {
    const get = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const set = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(lastFocusMinutes()).toBeNull();
    expect(() => rememberFocusMinutes(45)).not.toThrow();
    expect(get).toHaveBeenCalled();
    expect(set).toHaveBeenCalled();
  });

  it('says one plain line when a start fails', async () => {
    invoke.mockRejectedValue(new Error('refused'));
    const notify = vi.fn();
    focusPaletteItems(OFF, NOW, null, notify)[1].run();
    await vi.waitFor(() => expect(notify).toHaveBeenCalledWith("Couldn't start Focus. Try again."));
  });
});
