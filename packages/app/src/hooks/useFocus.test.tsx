// @vitest-environment jsdom
// The Focus hook: the view on mount, the focus://changed event, a failed read (no note, no crash), and the
// 30 s clock tick while Focus is on.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FocusView } from '../lib/focus';
import { useFocus, type FocusNow } from './useFocus';

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), handlers: [] as ((event: { payload: unknown }) => void)[] }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => {
    ipc.handlers.push(handler);
    return () => undefined;
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_000_000_000;
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
const ON: FocusView = { ...OFF, on: true, sessionId: 's1', startedAtMs: NOW, endsAtMs: NOW + 45 * 60_000, minutes: 45 };

let host: HTMLDivElement;
let root: Root;
let latest: FocusNow | null;
function Probe() {
  latest = useFocus();
  return null;
}

/** Let the mocked IPC promises settle (fake timers do not stop microtasks). */
async function settle() {
  await act(async () => {
    for (let i = 0; i < 5; i++) await Promise.resolve();
  });
}

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__ = {};
  ipc.invoke.mockReset();
  ipc.handlers.length = 0;
  latest = null;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
  vi.useRealTimers();
});

describe('useFocus', () => {
  it('reads the view on mount', async () => {
    ipc.invoke.mockResolvedValue(ON);
    act(() => root.render(<Probe />));
    await settle();
    expect(ipc.invoke).toHaveBeenCalledWith('focus_state');
    expect(latest?.view).toEqual(ON);
  });

  it('takes every focus://changed event', async () => {
    ipc.invoke.mockResolvedValue(ON);
    act(() => root.render(<Probe />));
    await settle();
    act(() => ipc.handlers.forEach((h) => h({ payload: OFF })));
    expect(latest?.view?.on).toBe(false);
  });

  it('leaves the view empty when the read fails (no note, no crash)', async () => {
    ipc.invoke.mockRejectedValue(new Error('no backend'));
    act(() => root.render(<Probe />));
    await settle();
    expect(latest?.view).toBeNull();
  });

  it('ticks the clock every 30 seconds while Focus is on', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    ipc.invoke.mockResolvedValue(ON);
    act(() => root.render(<Probe />));
    await settle();
    expect(latest?.now).toBe(NOW);
    // The clock moves with the timers: the tick at 30 s reads 30 s after the start.
    act(() => {
      vi.advanceTimersByTime(29_999);
    });
    expect(latest?.now).toBe(NOW);
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(latest?.now).toBe(NOW + 30_000);
  });

  it('does not tick while Focus is off', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    ipc.invoke.mockResolvedValue(OFF);
    act(() => root.render(<Probe />));
    await settle();
    vi.setSystemTime(NOW + 60_000);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(latest?.now).toBe(NOW);
  });
});
