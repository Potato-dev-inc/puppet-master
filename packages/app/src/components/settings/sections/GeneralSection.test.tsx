// @vitest-environment jsdom
// Settings > General: every control renders, saves the right partial patch, and destructive actions are confirmed.
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto, type Mounted } from '../test-utils';
import { generalRows } from './GeneralSection';

const mocks = vi.hoisted(() => ({
  install: { version: '0.9.0', isPackaged: true, platform: 'windows', uninstallAvailable: true, uninstallInstructions: 'Removes the app.', dataDir: 'C:\\data' },
  storage: { scope: 'project', project_path: 'C:\\work\\demo', storage_dir: 'C:\\work\\demo\\.puppet-master', event_log_path: 'C:\\work\\demo\\.puppet-master\\events.jsonl', event_count: 12, task_count: 3, lock_count: 1, exists: true },
  loop: { enabled: true, turns: 4, stalled_minutes: 20, panic_shortcut: false },
}));

vi.mock('@tauri-apps/api/path', () => ({ homeDir: vi.fn(async () => 'C:\\Users\\dev'), appConfigDir: vi.fn(async () => 'C:\\cfg'), join: vi.fn(async (...p: string[]) => p.join('\\')) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(async () => 'C:\\picked'), ask: vi.fn(async () => true) }));
vi.mock('../../../lib/settings', () => ({ resolveSettingsFilePath: vi.fn(async () => 'C:\\cfg\\settings.json') }));
vi.mock('../../../lib/tauri', () => ({
  tauri: {
    getAppInstallInfo: vi.fn(async () => mocks.install),
    getCoordinationStorageInfo: vi.fn(async () => mocks.storage),
    launchUninstall: vi.fn(async () => undefined),
  },
}));
vi.mock('../../../lib/loop-guard', () => ({
  tauriLoopApi: {
    settingsGet: vi.fn(async () => ({ ...mocks.loop })),
    settingsSet: vi.fn(async (next: unknown) => next),
  },
}));
vi.mock('../../../lib/cockpit-api', () => ({
  cockpitApi: { ambientGetPrefs: vi.fn(async () => ({ animated_tray_icon: true })), ambientSetAnimatedTray: vi.fn(async () => undefined) },
}));
vi.mock('../../../lib/memory-api', () => ({
  memoryApi: { globalEnabled: vi.fn(async () => true), setGlobalEnabled: vi.fn(async () => undefined) },
}));

import { tauri } from '../../../lib/tauri';
import { tauriLoopApi } from '../../../lib/loop-guard';
import { cockpitApi } from '../../../lib/cockpit-api';
import { memoryApi } from '../../../lib/memory-api';

let m: Mounted;

async function mount(overrides: Parameters<typeof mountRows>[1] = {}) {
  m = await mountRows(generalRows, overrides);
  await settle(5);
  return m;
}

beforeEach(() => vi.clearAllMocks());
afterEach(async () => {
  await m?.unmount();
  vi.useRealTimers();
});

const switchFor = (label: string) => m.host.querySelector<HTMLButtonElement>(`[role="switch"][aria-label="${label}"]`);

describe('General: every control renders', () => {
  it('shows every live control of the old General, Appearance, Sessions, Storage and Advanced tabs that belongs here', async () => {
    await mount();
    for (const label of [
      'Default project folder',
      'Theme',
      'Animated tray icon',
      'Ask before closing a working agent',
      'Warn when an agent seems stuck',
      'Stuck warning sensitivity',
      'Quiet for too long',
      'Global shortcut: stop all agents',
      'Away digest after',
      'Checkpoints',
      'Keep last N checkpoints',
      'Shared project memory',
      'Task board and locks',
      'Event log',
      'Version',
      'Settings file',
      'Uninstall Puppet Master',
    ]) {
      expect(hasText(m.host, label), `missing: ${label}`).toBe(true);
    }
    expect(buttonByText(m.host, 'Browse')).toBeTruthy();
    expect(buttonByText(m.host, 'Refresh')).toBeTruthy();
    expect(buttonByText(m.host, 'Check for updates')).toBeTruthy();
    expect(buttonByText(m.host, 'Export…')).toBeTruthy();
    expect(buttonByText(m.host, 'Uninstall…')).toBeTruthy();
  });

  it('has no Save button and no planned/placeholder controls', async () => {
    await mount();
    expect(buttonByText(m.host, 'Save changes')).toBeUndefined();
    expect(hasText(m.host, 'Planned')).toBe(false);
    expect(hasText(m.host, 'Not implemented')).toBe(false);
  });

  it('puts the destructive row in its own last list', async () => {
    await mount();
    const lists = [...m.host.querySelectorAll('section.st-group')];
    const last = lists[lists.length - 1];
    expect(last.getAttribute('data-danger')).toBe('true');
    expect(last.textContent).toContain('Uninstall Puppet Master');
    expect(lists.slice(0, -1).some((s) => s.textContent?.includes('Uninstall Puppet Master'))).toBe(false);
  });
});

describe('General: each control saves a partial patch', () => {
  it('Theme saves only {theme}', async () => {
    await mount();
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Theme')!, 'light');
    expect(m.env.patches).toEqual([{ theme: 'light' }]);
  });

  it('Ask before closing saves only {confirm_close_running_pane}', async () => {
    await mount();
    await click(switchFor('Ask before closing a working agent'));
    expect(m.env.patches).toEqual([{ confirm_close_running_pane: false }]);
  });

  it('Checkpoints saves immediately; Keep last N clamps on blur and is disabled when checkpoints are off', async () => {
    await mount();
    await click(switchFor('Checkpoints'));
    expect(m.env.patches).toEqual([{ checkpoints_enabled: false }]);
    await m.rerender();
    expect(byLabel<HTMLInputElement>(m.host, 'Keep last N checkpoints')!.disabled).toBe(true);
    await click(switchFor('Checkpoints'));
    await m.rerender();
    const keep = byLabel<HTMLInputElement>(m.host, 'Keep last N checkpoints')!;
    expect(keep.disabled).toBe(false);
    await typeInto(keep, '2');
    expect(m.env.patches).toHaveLength(2); // typing "2" on the way to "200" saved nothing
    await typeInto(keep, '9000');
    await blur(keep);
    expect(m.env.patches.at(-1)).toEqual({ checkpoints_keep: 500 });
  });

  it('Away digest minutes clamps on blur', async () => {
    await mount();
    const away = byLabel<HTMLInputElement>(m.host, 'Away digest after (minutes)')!;
    await typeInto(away, '-4');
    await blur(away);
    expect(m.env.patches).toEqual([{ away_minutes: 0 }]);
  });

  it('Default project folder saves after a pause through onProjectPathChange, not as a whole snapshot', async () => {
    vi.useFakeTimers();
    const onProjectPathChange = vi.fn(async () => undefined);
    await mount({ onProjectPathChange });
    const field = byLabel<HTMLInputElement>(m.host, 'Default project folder')!;
    await typeInto(field, 'C:\\work\\other');
    expect(onProjectPathChange).not.toHaveBeenCalled();
    await act(async () => void (await vi.advanceTimersByTimeAsync(700)));
    expect(onProjectPathChange).toHaveBeenCalledExactlyOnceWith('C:\\work\\other');
    expect(m.env.markSaved).toHaveBeenCalled();
  });

  it('Browse saves the picked folder', async () => {
    const onProjectPathChange = vi.fn(async () => undefined);
    await mount({ onProjectPathChange });
    await click(buttonByText(m.host, 'Browse'));
    await settle();
    expect(onProjectPathChange).toHaveBeenCalledExactlyOnceWith('C:\\picked');
  });

  it('a failed folder save shows an error on the row', async () => {
    vi.useFakeTimers();
    const onProjectPathChange = vi.fn(async () => {
      throw new Error('not a folder');
    });
    await mount({ onProjectPathChange });
    const field = byLabel<HTMLInputElement>(m.host, 'Default project folder')!;
    await typeInto(field, 'nope');
    await blur(field);
    await settle();
    expect(m.env.setError).toHaveBeenCalledWith('Not saved: not a folder');
  });

  it('Loop guard, animated tray and shared memory save through their own commands and tick the row', async () => {
    await mount();
    await click(switchFor('Global shortcut: stop all agents'));
    await settle();
    expect(tauriLoopApi.settingsSet).toHaveBeenCalledWith({ ...mocks.loop, panic_shortcut: true });
    await click(switchFor('Animated tray icon'));
    await settle();
    expect(cockpitApi.ambientSetAnimatedTray).toHaveBeenCalledWith(false);
    await click(switchFor('Shared project memory'));
    await settle();
    expect(memoryApi.setGlobalEnabled).toHaveBeenCalledWith(false);
    expect(m.env.markSaved).toHaveBeenCalledTimes(3);
    expect(m.env.patches).toEqual([]); // none of these touch settings.json
  });
});

describe('General: updates, export, uninstall', () => {
  it('Check for updates calls the update check; Download appears only when one is available', async () => {
    const onCheck = vi.fn();
    const onOpenRelease = vi.fn();
    await mount({ update: { onCheck, onOpenRelease } });
    await click(buttonByText(m.host, 'Check for updates'));
    expect(onCheck).toHaveBeenCalledTimes(1);
    expect(buttonByText(m.host, 'Download & install')).toBeUndefined();
    await m.unmount();
    await mount({ update: { onCheck, onOpenRelease, result: { currentVersion: '0.9.0', latestVersion: '1.0.0', updateAvailable: true, releaseUrl: 'x', releaseNotes: null, error: null } } });
    expect(hasText(m.host, 'Update available: v1.0.0')).toBe(true);
    await click(buttonByText(m.host, 'Download & install'));
    expect(onOpenRelease).toHaveBeenCalledTimes(1);
  });

  it('Export builds a file with the keys redacted', async () => {
    const created: Blob[] = [];
    URL.createObjectURL = vi.fn((b: Blob) => (created.push(b), 'blob:x'));
    URL.revokeObjectURL = vi.fn();
    await mount({ settings: { ...(await import('../test-utils')).BASE_SETTINGS, anthropic_api_key: 'sk-secret' } });
    const anchorClick = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
    await click(buttonByText(m.host, 'Export…'));
    anchorClick.mockRestore();
    expect(created).toHaveLength(1);
    const text = await new Promise<string>((resolve) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.readAsText(created[0]);
    });
    expect(text).toContain('"anthropic_api_key": "***"');
    expect(text).not.toContain('sk-secret');
  });

  it('Uninstall asks first; Cancel does nothing, confirm launches the uninstaller', async () => {
    await mount();
    m.env.confirm.mockResolvedValueOnce(false);
    await click(buttonByText(m.host, 'Uninstall…'));
    await settle();
    expect(m.env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Uninstall Puppet Master?', confirmLabel: 'Uninstall' }));
    expect(tauri.launchUninstall).not.toHaveBeenCalled();
    await click(buttonByText(m.host, 'Uninstall…'));
    await settle();
    expect(tauri.launchUninstall).toHaveBeenCalledTimes(1);
  });

  it('Uninstall is disabled with a plain reason in a dev build', async () => {
    mocks.install.uninstallAvailable = false;
    await mount();
    expect(buttonByText(m.host, 'Uninstall…')!.disabled).toBe(true);
    expect(hasText(m.host, 'Only available in an installed build')).toBe(true);
    mocks.install.uninstallAvailable = true;
  });
});
