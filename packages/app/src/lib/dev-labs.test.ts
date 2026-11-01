// @vitest-environment jsdom
// Development build: every Labs switch reads as on. The app answers once (dev_labs_override_active); the settings
// the window reads show the switches on, and the saved settings are never written with them.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
  const state = {
    devLabs: true as unknown, // what dev_labs_override_active answers; an Error makes the call fail
    server: {} as Record<string, unknown>,
    calls: [] as string[],
  };
  return { state };
});

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string, args: { patch?: Record<string, unknown> }) => {
    h.state.calls.push(cmd);
    if (cmd === 'dev_labs_override_active') {
      if (h.state.devLabs instanceof Error) throw h.state.devLabs;
      return h.state.devLabs;
    }
    if (cmd === 'settings_patch') {
      const { ui_mode: _owned, ...patch } = args.patch ?? {};
      h.state.server = { ...h.state.server, ...patch };
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

import { DEV_LABS_KEYS, withDevLabs, devLabsOverrideActive, setDevLabsOverrideForTests } from './dev-labs';
import { loadSettings, patchSettings } from './settings';

const win = window as unknown as Record<string, unknown>;

beforeEach(() => {
  win.__TAURI_INTERNALS__ = {};
  h.state.devLabs = true;
  h.state.server = { theme: 'dark', away_minutes: 5, missions_enabled: false, potato_bot_enabled: false };
  h.state.calls = [];
  setDevLabsOverrideForTests(false);
});
afterEach(() => {
  delete win.__TAURI_INTERNALS__;
  setDevLabsOverrideForTests(false);
});

describe('the development override, asked of the app once', () => {
  // A fresh copy of the module per test: the answer is remembered in the module.
  async function fresh() {
    vi.resetModules();
    return import('./dev-labs');
  }

  it('is on when the app says so, and the app is asked only once', async () => {
    const dl = await fresh();
    expect(await dl.probeDevLabs()).toBe(true);
    expect(dl.devLabsOverrideActive()).toBe(true);
    await dl.probeDevLabs();
    expect(h.state.calls.filter((c) => c === 'dev_labs_override_active')).toHaveLength(1);
  });

  it('is off when the app says no, and when the question fails', async () => {
    h.state.devLabs = false;
    expect(await (await fresh()).probeDevLabs()).toBe(false);
    h.state.devLabs = new Error('no command');
    expect(await (await fresh()).probeDevLabs()).toBe(false);
  });

  it('is off outside the desktop app, and asks nothing', async () => {
    delete win.__TAURI_INTERNALS__;
    const dl = await fresh();
    expect(await dl.probeDevLabs()).toBe(false);
    expect(h.state.calls).toEqual([]);
  });
});

describe('withDevLabs', () => {
  it('turns every Labs key on in a copy, and leaves the input alone', () => {
    setDevLabsOverrideForTests(true);
    const input = { potato_bot_enabled: false, theme: 'dark' };
    const out = withDevLabs(input) as Record<string, unknown>;
    for (const key of DEV_LABS_KEYS) expect(out[key], key).toBe(true);
    expect(out.theme).toBe('dark');
    expect(input).toEqual({ potato_bot_enabled: false, theme: 'dark' });
  });

  it('returns the same object when the override is off', () => {
    const input = { potato_bot_enabled: false };
    expect(withDevLabs(input)).toBe(input);
  });
});

describe('the settings window reads the switches on, and the saved file keeps them as stored', () => {
  it('loadSettings shows every Labs switch on while settings.json still says off', async () => {
    setDevLabsOverrideForTests(true);
    const loaded = await loadSettings();
    expect(loaded.missions_enabled).toBe(true);
    expect(loaded.potato_bot_enabled).toBe(true);
    expect(h.state.server.missions_enabled).toBe(false);
    expect(h.state.server.potato_bot_enabled).toBe(false);
  });

  it('patchSettings returns the switches on and writes only the keys it was given', async () => {
    setDevLabsOverrideForTests(true);
    const saved = await patchSettings({ away_minutes: 9 });
    expect(saved.missions_enabled).toBe(true);
    expect(h.state.server.away_minutes).toBe(9);
    expect(h.state.server.missions_enabled).toBe(false);
  });

  it('with the override off, the stored values are what the window reads', async () => {
    setDevLabsOverrideForTests(false);
    const loaded = await loadSettings();
    expect(loaded.missions_enabled).toBe(false);
    expect(loaded.potato_bot_enabled).toBe(false);
    expect(devLabsOverrideActive()).toBe(false);
  });
});
