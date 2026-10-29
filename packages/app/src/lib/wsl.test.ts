import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { PaneInfoSchema, PanePlaceSchema } from '@puppet-master/shared';
import {
  spawnPaneInLinux,
  tauriWslApi,
  wslCheck,
  wslPlaceSet,
  wslPlacesGet,
  wslStatus,
  type DistroView,
  type WslApi,
  type WslStatus,
} from './wsl';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined as never);
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

describe('wsl command wrappers', () => {
  it('wslStatus sends refresh and returns the status', async () => {
    const ready: WslStatus = { state: 'ready', distros: [{ name: 'Ubuntu', default: true, version: 2, state: 'Running' }] };
    invokeMock.mockResolvedValueOnce(ready as never);
    await expect(wslStatus(true)).resolves.toEqual(ready);
    expect(invokeMock).toHaveBeenCalledWith('wsl_status', { refresh: true });
  });

  it('wslCheck sends the distro name', async () => {
    await wslCheck('Ubuntu');
    expect(invokeMock).toHaveBeenCalledWith('wsl_check', { distro: 'Ubuntu' });
  });

  it('wslPlacesGet calls the places command with no args', async () => {
    await wslPlacesGet();
    expect(invokeMock).toHaveBeenCalledWith('wsl_places_get', undefined);
  });

  it('wslPlaceSet passes agentId and a null distro (back to Windows)', async () => {
    await wslPlaceSet('demo-agent', null);
    expect(invokeMock).toHaveBeenCalledWith('wsl_place_set', { agentId: 'demo-agent', distro: null });
    await wslPlaceSet('demo-agent', 'Ubuntu-Test');
    expect(invokeMock).toHaveBeenLastCalledWith('wsl_place_set', { agentId: 'demo-agent', distro: 'Ubuntu-Test' });
  });

  it('spawnPaneInLinux sends the args unchanged with the distro', async () => {
    invokeMock.mockResolvedValueOnce('pane-1' as never);
    const args = { agent_type: 'demo-agent', cwd: 'C:\\Users\\you\\shop', extra_args: ['--a', '$(touch x)'] };
    await expect(spawnPaneInLinux(args, 'Ubuntu-Test')).resolves.toBe('pane-1');
    expect(invokeMock).toHaveBeenCalledWith('spawn_pane_in_linux', { args, distro: 'Ubuntu-Test' });
  });

  it('refuses outside the desktop window without calling invoke', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await expect(wslStatus(false)).rejects.toThrow('Desktop shell required: wsl_status');
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('uses an injected api object instead of invoke', async () => {
    const fake: WslApi = {
      ...tauriWslApi,
      status: vi.fn(async () => ({ state: 'not_windows' }) as WslStatus),
      placeSet: vi.fn(async (agentId: string, distro: string | null) => ({
        agentId,
        label: 'Demo',
        distro,
        found: null,
        whyNot: null,
      })),
    };
    await expect(wslStatus(false, fake)).resolves.toEqual({ state: 'not_windows' });
    await expect(wslPlaceSet('demo-agent', 'Ubuntu', fake)).resolves.toMatchObject({ agentId: 'demo-agent', distro: 'Ubuntu' });
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe('wsl JSON shapes (mirror wsl/model.rs goldens)', () => {
  it('reads every WslStatus variant with the Rust tags', () => {
    const samples: WslStatus[] = [
      JSON.parse('{"state":"not_windows"}'),
      JSON.parse('{"state":"not_installed","command":"wsl --install"}'),
      JSON.parse('{"state":"no_distro","command":"wsl --install -d Ubuntu"}'),
      JSON.parse('{"state":"ready","distros":[{"name":"Ubuntu","default":true,"version":2,"state":"Running"}]}'),
      JSON.parse('{"state":"broken","line":"boom"}'),
    ];
    expect(samples.map((s) => s.state)).toEqual(['not_windows', 'not_installed', 'no_distro', 'ready', 'broken']);
  });

  it('reads a DistroView with camelCase keys and no PATH, uid or home', () => {
    const view: DistroView = JSON.parse(
      '{"name":"Ubuntu-Test","version":2,"default":false,"checkedAtMs":1700000000000,"git":"2.43.0","interop":true,"networking":null,"agents":[{"agentId":"demo-agent","label":"Demo Agent","distro":null,"found":null,"whyNot":null}]}',
    );
    expect(Object.keys(view).sort()).toEqual(['agents', 'checkedAtMs', 'default', 'git', 'interop', 'name', 'networking', 'version']);
    expect(view.agents[0]).toMatchObject({ agentId: 'demo-agent', distro: null, found: null, whyNot: null });
  });

  it('accepts the Rust PanePlace golden in the shared schema, and PaneInfo with and without it', () => {
    const place = {
      kind: 'linux',
      distro: 'Ubuntu',
      cwd: '/mnt/d/work/shop',
      mount_root: '/mnt/',
      unc_root: '\\\\wsl.localhost\\Ubuntu',
      home_windows: '\\\\wsl.localhost\\Ubuntu\\home\\you',
    };
    expect(PanePlaceSchema.safeParse(place).success).toBe(true);
    expect(PanePlaceSchema.safeParse({ ...place, warnings: ['Line endings differ.'] }).success).toBe(true);
    expect(PanePlaceSchema.safeParse({ kind: 'linux' }).success).toBe(false);

    const pane = {
      id: 'p1',
      agent_type: 'demo_agent',
      pid: 42,
      status: 'running',
      created_at: 1,
      last_output_at: null,
      cwd: 'D:\\work\\shop',
      cols: 120,
      rows: 30,
    };
    expect(PaneInfoSchema.parse(pane)).not.toHaveProperty('place');
    expect(PaneInfoSchema.parse({ ...pane, place }).place).toMatchObject({ distro: 'Ubuntu', cwd: '/mnt/d/work/shop' });
  });
});
