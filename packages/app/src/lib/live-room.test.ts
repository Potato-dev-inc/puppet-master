// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  durationLabel,
  liveRoomApi,
  LIVE_STATE_EVENT,
  maskLink,
  minutesLeft,
  onLiveState,
  type LiveStatus,
} from './live-room';

const LINK = 'https://relay.example.test/r/#aaaaaaaaaaaaaaaaaaaaaaaaaa.c2VjcmV0LXRva2VuLWhlcmU.Y29udGVudC1rZXktc2VjcmV0LWhlcmUtMzItYnl0ZXM';

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('liveRoomApi argument shapes', () => {
  it('preview sends paneIds', async () => {
    invoke.mockResolvedValue({ panes: [], items: [], hidden: 0 });
    await liveRoomApi.preview(['p1', 'p2']);
    expect(invoke).toHaveBeenCalledWith('live_room_preview', { paneIds: ['p1', 'p2'] });
  });

  it('start sends the request under req', async () => {
    invoke.mockResolvedValue({ endsAtMs: 1000 });
    const req = { paneIds: ['p1'], title: 'Live Room', minutes: 60, maxViewers: 10 };
    const out = await liveRoomApi.start(req);
    expect(invoke).toHaveBeenCalledWith('live_room_start', { req });
    expect(out).toEqual({ endsAtMs: 1000 });
  });

  it('stop, pause, resume and status call their commands with no arguments', async () => {
    invoke.mockResolvedValue(undefined);
    await liveRoomApi.stop();
    await liveRoomApi.pause();
    await liveRoomApi.resume();
    expect(invoke.mock.calls).toEqual([['live_room_stop'], ['live_room_pause'], ['live_room_resume']]);
    invoke.mockResolvedValue({ live: false });
    await liveRoomApi.status();
    expect(invoke).toHaveBeenLastCalledWith('live_room_status');
  });

  it('copyLink returns the link string', async () => {
    invoke.mockResolvedValue(LINK);
    expect(await liveRoomApi.copyLink()).toBe(LINK);
    expect(invoke).toHaveBeenCalledWith('live_room_copy_link');
  });

  it('configGet and configSet use the config shape', async () => {
    const cfg = { relay: 'https://relay.example.test', minutes: 60, maxViewers: 10 };
    invoke.mockResolvedValueOnce(cfg);
    expect(await liveRoomApi.configGet()).toEqual(cfg);
    expect(invoke).toHaveBeenLastCalledWith('live_room_config_get');
    invoke.mockResolvedValueOnce(undefined);
    await liveRoomApi.configSet(cfg);
    expect(invoke).toHaveBeenLastCalledWith('live_room_config_set', { cfg });
  });

  it('errors pass through as their message', async () => {
    invoke.mockRejectedValueOnce('Live Room needs a relay address.');
    await expect(liveRoomApi.start({ paneIds: [], title: '', minutes: 60, maxViewers: 10 })).rejects.toThrow(
      'Live Room needs a relay address.',
    );
    invoke.mockRejectedValueOnce(new Error('boom'));
    await expect(liveRoomApi.stop()).rejects.toThrow('boom');
  });

  it('refuses outside the desktop shell without calling invoke', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await expect(liveRoomApi.pause()).rejects.toThrow('Desktop shell required: live_room_pause');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('onLiveState', () => {
  it('listens to live://state and passes the payload', async () => {
    let handler: ((e: { payload: LiveStatus }) => void) | undefined;
    const unlisten = vi.fn();
    listen.mockImplementation((_name: string, h: typeof handler) => {
      handler = h;
      return Promise.resolve(unlisten);
    });
    const seen: LiveStatus[] = [];
    const stop = await onLiveState((s) => seen.push(s));
    expect(listen).toHaveBeenCalledWith(LIVE_STATE_EVENT, expect.any(Function));
    expect(LIVE_STATE_EVENT).toBe('live://state');
    const status: LiveStatus = { live: true, paused: false, viewers: 2, endsAtMs: 5, paneIds: ['p1'], hidden: 0, stopped: null };
    handler?.({ payload: status });
    expect(seen).toEqual([status]);
    expect(stop).toBe(unlisten);
  });

  it('is a no-op outside the desktop shell', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    const stop = await onLiveState(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});

describe('maskLink', () => {
  it('keeps scheme, host and path and hides the fragment', () => {
    const masked = maskLink(LINK);
    expect(masked).toBe('https://relay.example.test/r/#••••');
    expect(masked).not.toContain('aaaaaaaa');
    expect(masked).not.toContain('secret');
  });

  it('keeps a relay path prefix', () => {
    expect(maskLink('https://example.test/pm/r/#abc.def.ghi')).toBe('https://example.test/pm/r/#••••');
  });

  it('turns text that is not a URL into a plain mask', () => {
    expect(maskLink('not a link')).toBe('••••');
    expect(maskLink('')).toBe('••••');
  });
});

describe('minutesLeft', () => {
  const MIN = 60_000;

  it('rounds up so 0 shows only when time is up', () => {
    const now = 1_000_000;
    expect(minutesLeft(now + 41 * MIN, now)).toBe(41);
    expect(minutesLeft(now + 41 * MIN + 30_000, now)).toBe(42);
    expect(minutesLeft(now + 1, now)).toBe(1);
    expect(minutesLeft(now, now)).toBe(0);
  });

  it('never goes negative', () => {
    expect(minutesLeft(1000, 999_999)).toBe(0);
  });
});

describe('durationLabel', () => {
  it('writes the length choices as plain words', () => {
    expect([30, 60, 120, 240, 480].map(durationLabel)).toEqual(['30 min', '1 h', '2 h', '4 h', '8 h']);
    expect(durationLabel(90)).toBe('1 h 30 min');
  });
});
