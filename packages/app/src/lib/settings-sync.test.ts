// @vitest-environment jsdom
// Fresh state: when the Settings window must reload, and who follows a theme change.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const bus = vi.hoisted(() => ({ handlers: new Map<string, (e: { payload: unknown }) => void>(), unlisten: vi.fn() }));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload: unknown }) => void) => {
    bus.handlers.set(name, cb);
    return bus.unlisten;
  }),
}));

import { subscribeSettingsRefresh, subscribeSettingsTheme } from './settings-sync';

const win = window as unknown as Record<string, unknown>;
const emit = (name: string, payload: unknown) => bus.handlers.get(name)?.({ payload });
const tick = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  bus.handlers.clear();
  bus.unlisten.mockClear();
  win.__TAURI_INTERNALS__ = {};
});
afterEach(() => {
  delete win.__TAURI_INTERNALS__;
});

describe('subscribeSettingsRefresh', () => {
  it('refreshes on window focus and when the page becomes visible again', () => {
    const onRefresh = vi.fn();
    const off = subscribeSettingsRefresh(onRefresh);
    window.dispatchEvent(new Event('focus'));
    expect(onRefresh).toHaveBeenLastCalledWith({ reason: 'focus' });
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
    expect(onRefresh).toHaveBeenCalledTimes(1); // hidden: nothing to refresh yet
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
    expect(onRefresh).toHaveBeenCalledTimes(2);
    off();
    window.dispatchEvent(new Event('focus'));
    expect(onRefresh).toHaveBeenCalledTimes(2);
  });

  it('refreshes on settings://changed (another window saved) and settings://show (window brought back)', async () => {
    const onRefresh = vi.fn();
    const off = subscribeSettingsRefresh(onRefresh);
    await tick();
    emit('settings://changed', { theme: 'light' });
    expect(onRefresh).toHaveBeenLastCalledWith({ reason: 'changed' });
    emit('settings://show', { tab: 'bridge' });
    expect(onRefresh).toHaveBeenLastCalledWith({ reason: 'show', tab: 'bridge' });
    emit('settings://show', 'keys');
    expect(onRefresh).toHaveBeenLastCalledWith({ reason: 'show', tab: 'keys' });
    emit('settings://show', null);
    expect(onRefresh).toHaveBeenLastCalledWith({ reason: 'show', tab: undefined });
    off();
    expect(bus.unlisten).toHaveBeenCalledTimes(2);
  });

  it('works without Tauri (browser): only focus and visibility', () => {
    delete win.__TAURI_INTERNALS__;
    const onRefresh = vi.fn();
    const off = subscribeSettingsRefresh(onRefresh);
    expect(bus.handlers.size).toBe(0);
    window.dispatchEvent(new Event('focus'));
    expect(onRefresh).toHaveBeenCalledTimes(1);
    off();
  });
});

describe('subscribeSettingsTheme', () => {
  it('reports a theme in settings://changed, and ignores events without one', async () => {
    const onTheme = vi.fn();
    const off = subscribeSettingsTheme(onTheme);
    await tick();
    emit('settings://changed', { away_minutes: 9 });
    emit('settings://changed', { theme: 'blue' });
    emit('settings://changed', null);
    expect(onTheme).not.toHaveBeenCalled();
    emit('settings://changed', { theme: 'light' });
    expect(onTheme).toHaveBeenCalledExactlyOnceWith('light');
    off();
    expect(bus.unlisten).toHaveBeenCalledTimes(1);
  });
});
