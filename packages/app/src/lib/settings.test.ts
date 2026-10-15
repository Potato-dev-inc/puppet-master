// @vitest-environment jsdom
// patchSettings: partial writes through the Rust `settings_patch` command.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const state = { server: {} as Record<string, unknown>, calls: [] as Array<{ cmd: string; args: unknown }> };
  const mergePatch = (target: Record<string, unknown>, patch: Record<string, unknown>) => {
    const out = { ...target };
    for (const [k, v] of Object.entries(patch)) {
      if (v === null) delete out[k];
      else if (v && typeof v === 'object' && !Array.isArray(v) && out[k] && typeof out[k] === 'object') out[k] = mergePatch(out[k] as Record<string, unknown>, v as Record<string, unknown>);
      else out[k] = v;
    }
    return out;
  };
  return { state, mergePatch };
});

vi.mock('@tauri-apps/api/core', () => ({
  // The Rust command, in miniature: merge under "a lock", drop ui_mode.
  invoke: vi.fn(async (cmd: string, args: { patch?: Record<string, unknown> }) => {
    h.state.calls.push({ cmd, args });
    if (cmd === 'settings_patch') {
      const { ui_mode: _owned, ...patch } = args.patch ?? {};
      h.state.server = h.mergePatch(h.state.server, patch);
      return h.state.server;
    }
    if (cmd === 'sync_public_settings') return null;
    throw new Error(`unexpected ${cmd}`);
  }),
}));
vi.mock('@tauri-apps/api/path', () => ({ appConfigDir: vi.fn(async () => 'C:\\cfg'), join: vi.fn(async (...p: string[]) => p.join('\\')) }));
vi.mock('@tauri-apps/plugin-store', () => ({
  LazyStore: class {
    async get() {
      return h.state.server;
    }
    async set(_k: string, v: Record<string, unknown>) {
      h.state.server = v;
    }
    async save() {}
  },
}));

import { invoke } from '@tauri-apps/api/core';
import { loadSettings, mergePatch, patchSettings } from './settings';

const win = window as unknown as Record<string, unknown>;

beforeEach(() => {
  h.state.server = { theme: 'dark', away_minutes: 5, ui_mode: 'individual', checkpoints_keep: 50 };
  h.state.calls = [];
  win.__TAURI_INTERNALS__ = {};
  delete document.documentElement.dataset.theme;
  vi.mocked(invoke).mockClear();
});
afterEach(() => {
  delete win.__TAURI_INTERNALS__;
});

describe('patchSettings in the app', () => {
  it('sends only the changed keys to settings_patch', async () => {
    await patchSettings({ away_minutes: 12 });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('settings_patch', { patch: { away_minutes: 12 } });
  });

  it('turns undefined into null so Rust removes the key', async () => {
    await patchSettings({ project_path: undefined, public_pwa_url: null });
    expect(vi.mocked(invoke).mock.calls[0][1]).toEqual({ patch: { project_path: null, public_pwa_url: null } });
  });

  it('clamps the values it knows the range of before sending', async () => {
    await patchSettings({ sidebar_width: 5000, mobile_input_delay_ms: 3, dev_server_port: 80 });
    expect(vi.mocked(invoke).mock.calls[0][1]).toEqual({ patch: { sidebar_width: 800, mobile_input_delay_ms: 50, dev_server_port: 1024 } });
  });

  it('applies the theme in this window when the theme was patched, and only then', async () => {
    await patchSettings({ theme: 'light' });
    expect(document.documentElement.dataset.theme).toBe('light');
    document.documentElement.dataset.theme = 'dark';
    await patchSettings({ away_minutes: 1 });
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('returns the settings as saved, with defaults filled in', async () => {
    const saved = await patchSettings({ checkpoints_keep: 80 });
    expect(saved.checkpoints_keep).toBe(80);
    expect(saved.theme).toBe('dark');
    expect(saved.confirm_close_running_pane).toBe(true);
  });

  it('two windows holding the same old copy do not clobber each other', async () => {
    const snapshotA = await loadSettings();
    const snapshotB = await loadSettings();
    expect(snapshotA.theme).toBe('dark');
    expect(snapshotB.away_minutes).toBe(5);
    await patchSettings({ theme: 'light' }); // window A saves its change
    await patchSettings({ away_minutes: 15 }); // window B saves its change, still holding the old theme
    const fresh = await loadSettings();
    expect(fresh.theme).toBe('light');
    expect(fresh.away_minutes).toBe(15);
  });

  it('never writes ui_mode (set_ui_mode owns it), even if a stale copy sends it', async () => {
    await patchSettings({ ui_mode: 'orchestration', theme: 'light' });
    expect(h.state.server.ui_mode).toBe('individual');
    expect(h.state.server.theme).toBe('light');
  });

  it('rejects when the write fails, so the row can show the error', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk is full'));
    await expect(patchSettings({ away_minutes: 2 })).rejects.toThrow('disk is full');
  });
});

describe('patchSettings in a plain browser (no Tauri)', () => {
  it('merges into the stored settings and keeps every other key', async () => {
    delete win.__TAURI_INTERNALS__;
    await patchSettings({ away_minutes: 9 });
    expect(invoke).not.toHaveBeenCalledWith('settings_patch', expect.anything());
    expect(h.state.server).toEqual({ theme: 'dark', away_minutes: 9, ui_mode: 'individual', checkpoints_keep: 50 });
  });
});

describe('mergePatch', () => {
  it('merges objects, removes null keys and replaces arrays', () => {
    expect(mergePatch({ a: 1, b: { c: 1, d: 2 }, l: [1, 2], x: 'y' }, { b: { c: 9 }, l: [3], x: null })).toEqual({ a: 1, b: { c: 9, d: 2 }, l: [3] });
  });
});
