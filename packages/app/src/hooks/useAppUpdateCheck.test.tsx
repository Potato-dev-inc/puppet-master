// @vitest-environment jsdom
// The desktop update check runs only inside the desktop app with a known version. Outside it (the phone
// PWA in a browser) `get_app_install_info` falls back to 0.0.0, and the page used to say "New version
// available ... (you have v0.0.0)" on every phone.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  info: vi.fn(),
  check: vi.fn(),
}));
vi.mock('../lib/tauri', () => ({ tauri: { getAppInstallInfo: mocks.info, openExternalUrl: vi.fn() } }));
vi.mock('../lib/app-update', () => ({ checkForAppUpdate: mocks.check }));

import { useAppUpdateCheck, type AppUpdateState } from './useAppUpdateCheck';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
let last: AppUpdateState;
function Probe() {
  last = useAppUpdateCheck(true);
  return null;
}
const update = { currentVersion: '0.0.0', latestVersion: '0.2.1', updateAvailable: true, releaseUrl: 'https://example.invalid/r' };
const info = (version: string, platform = 'windows') => ({ version, isPackaged: true, platform, uninstallAvailable: true, uninstallInstructions: '', dataDir: '' });

async function mount() {
  await act(async () => root.render(<Probe />));
  await act(async () => {
    for (let i = 0; i < 5; i++) await Promise.resolve();
  });
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  mocks.info.mockReset();
  mocks.check.mockReset().mockResolvedValue(update);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
});

describe('useAppUpdateCheck', () => {
  it('outside the desktop app (the phone PWA in a browser) never checks and never offers an update', async () => {
    mocks.info.mockResolvedValue(info('0.0.0', 'browser'));
    await mount();
    expect(mocks.check).not.toHaveBeenCalled();
    expect(last.result?.updateAvailable ?? false).toBe(false);
  });

  it('inside the app, an unknown or 0.0.0 version is never compared with the latest release', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    for (const version of ['0.0.0', '']) {
      mocks.info.mockResolvedValue(info(version));
      await mount();
    }
    expect(mocks.check).not.toHaveBeenCalled();
    expect(last.result?.updateAvailable ?? false).toBe(false);
  });

  it('inside the app with a real version, it checks', async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    mocks.info.mockResolvedValue(info('0.2.0'));
    mocks.check.mockResolvedValue({ ...update, currentVersion: '0.2.0' });
    await mount();
    expect(mocks.check).toHaveBeenCalledWith('0.2.0');
    expect(last.result?.updateAvailable).toBe(true);
  });
});
