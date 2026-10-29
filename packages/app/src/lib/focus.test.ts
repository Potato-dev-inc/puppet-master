import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  FOCUS_EVENT,
  FOCUS_MAX_MINUTES,
  FOCUS_MIN_MINUTES,
  FOCUS_PRESETS,
  type FocusView,
  type FocusSettings,
  heldText,
  focusEnd,
  focusExtend,
  focusStart,
  focusState,
  focusSettingsSet,
  focusSummaryDismiss,
  leftText,
  onFocusChanged,
} from './focus';

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

const MIN = 60_000;

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

const ON: FocusView = {
  ...OFF,
  on: true,
  sessionId: 'fc-0123456789ab',
  startedAtMs: 1_000_000,
  endsAtMs: 1_000_000 + 45 * MIN,
  minutes: 45,
  held: { run_done: 2, run_failed: 1, companion: 0, review: 0, night: 0 },
  heldTotal: 3,
  waitingQuiet: 2,
  waitingUrgent: 1,
};

beforeEach(() => {
  setTauri(false);
});

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  setTauri(false);
});

describe('Focus constants', () => {
  it('offers 25, 45 and 90 minutes, and clamps to 5..240', () => {
    expect([...FOCUS_PRESETS]).toEqual([25, 45, 90]);
    expect(FOCUS_MIN_MINUTES).toBe(5);
    expect(FOCUS_MAX_MINUTES).toBe(240);
  });
});

describe('leftText', () => {
  const now = 10_000_000;

  it('reads whole minutes left', () => {
    expect(leftText(now + 31 * MIN, now)).toBe('31 min left');
    expect(leftText(now + 90 * MIN, now)).toBe('90 min left');
  });

  it('rounds down, so it never promises more than is left', () => {
    expect(leftText(now + 30 * MIN + 30_000, now)).toBe('30 min left');
  });

  it('reads "1 min left" for exactly one minute, and "under 1 min left" below that', () => {
    expect(leftText(now + MIN, now)).toBe('1 min left');
    expect(leftText(now + MIN - 1, now)).toBe('under 1 min left');
    expect(leftText(now + 30_000, now)).toBe('under 1 min left');
  });

  it('reads "under 1 min left" at and past the end time', () => {
    expect(leftText(now, now)).toBe('under 1 min left');
    expect(leftText(now - 5 * MIN, now)).toBe('under 1 min left');
  });
});

describe('heldText', () => {
  it('says nothing held, one update, or a count', () => {
    expect(heldText({ heldTotal: 0 })).toBe('Nothing held yet');
    expect(heldText({ heldTotal: 1 })).toBe('Holding 1 update');
    expect(heldText({ heldTotal: 3 })).toBe('Holding 3 updates');
  });
});

describe('Focus wrappers over invoke', () => {
  it('reads the view with focus_state and no arguments', async () => {
    invoke.mockResolvedValue(OFF);
    await expect(focusState()).resolves.toEqual(OFF);
    expect(invoke).toHaveBeenCalledWith('focus_state');
  });

  it('starts a Focus with the minutes as `minutes`', async () => {
    invoke.mockResolvedValue(ON);
    await expect(focusStart(45)).resolves.toEqual(ON);
    expect(invoke).toHaveBeenCalledWith('focus_start', { minutes: 45 });
  });

  it('extends, ends and dismisses with their command names and no arguments', async () => {
    invoke.mockResolvedValue(ON);
    await focusExtend();
    expect(invoke).toHaveBeenLastCalledWith('focus_extend');
    await focusEnd();
    expect(invoke).toHaveBeenLastCalledWith('focus_end');
    invoke.mockResolvedValue(undefined);
    await focusSummaryDismiss();
    expect(invoke).toHaveBeenLastCalledWith('focus_summary_dismiss');
  });

  it('saves the settings as `settings` in snake_case', async () => {
    const settings: FocusSettings = { default_minutes: 90, quiet_blocking: false };
    invoke.mockResolvedValue({ ...OFF, settings });
    await expect(focusSettingsSet(settings)).resolves.toMatchObject({ settings });
    expect(invoke).toHaveBeenCalledWith('focus_settings_set', { settings: { default_minutes: 90, quiet_blocking: false } });
  });

  it('lets the Rust refusal reach the caller unchanged', async () => {
    invoke.mockRejectedValue('Pick between 5 and 240 minutes.');
    await expect(focusStart(3)).rejects.toBe('Pick between 5 and 240 minutes.');
  });
});

describe('onFocusChanged', () => {
  it('is a no-op outside the desktop shell', async () => {
    const off = await onFocusChanged(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => off()).not.toThrow();
  });

  it('listens to focus://changed and passes the payload to the handler', async () => {
    setTauri(true);
    const unlisten = vi.fn();
    let captured: ((event: { payload: FocusView }) => void) | undefined;
    listen.mockImplementation(async (_name: string, cb: (event: { payload: FocusView }) => void) => {
      captured = cb;
      return unlisten;
    });
    const handler = vi.fn();

    const off = await onFocusChanged(handler);
    expect(FOCUS_EVENT).toBe('focus://changed');
    expect(listen).toHaveBeenCalledWith('focus://changed', expect.any(Function));

    captured?.({ payload: ON });
    expect(handler).toHaveBeenCalledWith(ON);

    off();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
