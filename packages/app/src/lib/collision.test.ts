// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const unlisten = vi.fn();
const listen = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  type CollisionView,
  collisionIgnore,
  collisionList,
  collisionSettingsGet,
  collisionSettingsSet,
  isLoud,
  onCollisionChanged,
  otherSide,
  tauriCollisionApi,
} from './collision';

const ID = 'ca-0123456789ab';

function view(over: Partial<CollisionView> = {}): CollisionView {
  return {
    id: ID,
    level: 'clash',
    relation: 'copies',
    title: 'fake_a and fake_b both changed src/auth.ts',
    detail: 'Lines 40-80 overlap.',
    advice: 'Finish fake_a first. It started first.',
    place: 'In separate copies.',
    footer: 'Nothing was paused. You decide.',
    sides: [
      { paneId: 'pane-a', label: 'fake_a', working: true },
      { paneId: 'pane-b', label: 'fake_b', working: false },
    ],
    files: [{ path: 'src/auth.ts', text: 'Lines 40-80' }],
    more: 0,
    firstMs: 1000,
    lastMs: 2000,
    ...over,
  };
}

function setTauri(on: boolean): void {
  if (on) {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  }
}

beforeEach(() => {
  setTauri(true);
  invoke.mockReset();
  listen.mockReset();
  unlisten.mockReset();
});

afterEach(() => {
  setTauri(false);
});

describe('collision commands', () => {
  it('collisionList calls collision_list and returns the rows', async () => {
    invoke.mockResolvedValueOnce([view()]);
    const rows = await collisionList();
    expect(invoke.mock.calls[0][0]).toBe('collision_list');
    expect(rows).toHaveLength(1);
    expect(rows[0].id).toBe(ID);
  });

  it('collisionIgnore sends the id as `id`', async () => {
    invoke.mockResolvedValueOnce(undefined);
    await collisionIgnore(ID);
    expect(invoke).toHaveBeenCalledWith('collision_ignore', { id: ID });
  });

  it('collisionSettingsGet calls collision_settings_get', async () => {
    invoke.mockResolvedValueOnce({ enabled: true });
    expect(await collisionSettingsGet()).toEqual({ enabled: true });
    expect(invoke.mock.calls[0][0]).toBe('collision_settings_get');
  });

  it('collisionSettingsSet sends `settings` and returns what Rust saved', async () => {
    invoke.mockResolvedValueOnce({ enabled: false });
    expect(await collisionSettingsSet({ enabled: false })).toEqual({ enabled: false });
    expect(invoke).toHaveBeenCalledWith('collision_settings_set', { settings: { enabled: false } });
  });

  it('a command outside the desktop shell fails with the command name and never invokes', async () => {
    setTauri(false);
    await expect(collisionList()).rejects.toThrow('Desktop shell required: collision_list');
    await expect(collisionIgnore(ID)).rejects.toThrow('Desktop shell required: collision_ignore');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('tauriCollisionApi exposes the same commands', async () => {
    invoke.mockResolvedValueOnce([]);
    expect(await tauriCollisionApi.list()).toEqual([]);
    expect(invoke.mock.calls[0][0]).toBe('collision_list');
    invoke.mockResolvedValueOnce(undefined);
    await tauriCollisionApi.ignore(ID);
    expect(invoke).toHaveBeenLastCalledWith('collision_ignore', { id: ID });
  });
});

describe('onCollisionChanged', () => {
  it('listens to collision://changed and calls back on each event', async () => {
    let handler: ((e: { payload: unknown }) => void) | undefined;
    listen.mockImplementationOnce(async (_name: string, h: (e: { payload: unknown }) => void) => {
      handler = h;
      return unlisten;
    });
    const cb = vi.fn();
    const off = await onCollisionChanged(cb);
    expect(listen.mock.calls[0][0]).toBe('collision://changed');
    handler?.({ payload: {} });
    handler?.({ payload: {} });
    expect(cb).toHaveBeenCalledTimes(2);
    expect(unlisten).not.toHaveBeenCalled();
    off();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('the returned function is the Tauri unlisten', async () => {
    listen.mockResolvedValueOnce(unlisten);
    const off = await tauriCollisionApi.onChanged(() => {});
    expect(off).toBe(unlisten);
  });

  it('outside the desktop shell it does nothing and registers no listener', async () => {
    setTauri(false);
    const off = await onCollisionChanged(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => off()).not.toThrow();
  });
});

describe('isLoud', () => {
  it('is true for clash and shared_folder only', () => {
    expect(isLoud({ level: 'clash' })).toBe(true);
    expect(isLoud({ level: 'shared_folder' })).toBe(true);
    expect(isLoud({ level: 'same_file' })).toBe(false);
  });
});

describe('otherSide', () => {
  it('returns the side that is not the given pane, in either order', () => {
    const v = view();
    expect(otherSide(v, 'pane-a')?.paneId).toBe('pane-b');
    expect(otherSide(v, 'pane-b')?.paneId).toBe('pane-a');
    expect(otherSide(v, 'pane-b')?.label).toBe('fake_a');
  });

  it('is undefined for a pane that is not part of the alert', () => {
    expect(otherSide(view(), 'pane-c')).toBeUndefined();
  });
});
