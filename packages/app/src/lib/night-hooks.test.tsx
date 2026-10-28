import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EMPTY_STATE, RUNNING_STATE } from '../dev/night-fixtures';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const listen = vi.fn((name: string, handler: Handler) => {
  handlers.set(name, handler);
  return Promise.resolve(() => {
    handlers.delete(name);
  });
});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...(args as [string, Handler])) }));

import { type NightHook, NIGHT_POLL_MS, useNight, useNightEnabled } from './night';

let latest: NightHook | undefined;
function Probe() {
  latest = useNight();
  return null;
}

let enabledNow = false;
function EnabledProbe() {
  enabledNow = useNightEnabled();
  return null;
}

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setTauri(true);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  latest = undefined;
  setTauri(false);
});

async function renderProbe(): Promise<void> {
  await act(async () => root.render(<Probe />));
}

describe('useNight', () => {
  it('reads the state on mount and gives it to the page', async () => {
    invoke.mockResolvedValue(EMPTY_STATE);
    await renderProbe();
    expect(invoke).toHaveBeenCalledWith('night_state', undefined);
    expect(latest?.state).toEqual(EMPTY_STATE);
    expect(latest?.error).toBeNull();
  });

  it('reads again when night://changed fires', async () => {
    invoke.mockResolvedValueOnce(EMPTY_STATE).mockResolvedValueOnce(RUNNING_STATE);
    await renderProbe();
    expect(latest?.state?.run).toBeNull();
    expect(handlers.has('night://changed')).toBe(true);

    await act(async () => handlers.get('night://changed')?.({ payload: { rev: 2, state: 'running' } }));

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(latest?.state).toEqual(RUNNING_STATE);
  });

  it('reads again every 30 seconds while the run is running', async () => {
    vi.useFakeTimers();
    invoke.mockResolvedValue(RUNNING_STATE);
    await renderProbe();
    expect(invoke).toHaveBeenCalledTimes(1);

    await act(async () => vi.advanceTimersByTime(NIGHT_POLL_MS - 1));
    expect(invoke).toHaveBeenCalledTimes(1);
    await act(async () => vi.advanceTimersByTime(1));
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('does not poll when no run is running', async () => {
    vi.useFakeTimers();
    invoke.mockResolvedValue(EMPTY_STATE);
    await renderProbe();
    await act(async () => vi.advanceTimersByTime(NIGHT_POLL_MS * 3));
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it('keeps the plain sentence of a failed read', async () => {
    invoke.mockRejectedValue({ code: 'SAVE_FAILED', message: 'Could not read the night.', recoverable: true, context: null });
    await renderProbe();
    expect(latest?.state).toBeNull();
    expect(latest?.error).toBe('Could not read the night.');
  });

  it('does not read outside the desktop shell', async () => {
    setTauri(false);
    await renderProbe();
    expect(invoke).not.toHaveBeenCalled();
    expect(latest?.state).toBeNull();
    expect(latest?.error).toBeNull();
  });
});

describe('useNightEnabled', () => {
  it('reads the Labs switch and reads again when Settings change', async () => {
    invoke.mockResolvedValueOnce({ ...EMPTY_STATE, enabled: false }).mockResolvedValueOnce({ ...EMPTY_STATE, enabled: true });
    await act(async () => root.render(<EnabledProbe />));
    expect(enabledNow).toBe(false);

    await act(async () => handlers.get('settings://changed')?.({ payload: null }));

    expect(enabledNow).toBe(true);
  });
});
