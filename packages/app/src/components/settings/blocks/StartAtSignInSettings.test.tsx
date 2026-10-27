// @vitest-environment jsdom
// Settings > General: "Start Puppet Master when I sign in" reads the OS switch, shows what the OS reports after a
// change, and says so when the change fails. The Tauri commands are mocked; the real plugin is never called.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StartAtSignInSettings, START_AT_SIGN_IN_HINT, START_AT_SIGN_IN_LABEL } from './StartAtSignInSettings';

vi.mock('../../../lib/tauri', () => ({
  tauri: {
    autostartGet: vi.fn(),
    autostartSet: vi.fn(),
  },
}));

import { tauri } from '../../../lib/tauri';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const toggle = () => host.querySelector<HTMLButtonElement>('button[role="switch"]');

async function flush() {
  for (let i = 0; i < 4; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function mount(onSaved?: () => void) {
  await act(async () => {
    root.render(<StartAtSignInSettings onSaved={onSaved} />);
  });
  await flush();
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(tauri.autostartGet).mockReset();
  vi.mocked(tauri.autostartSet).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('StartAtSignInSettings', () => {
  it('shows the label and the hint, and reads the OS switch once', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    await mount();
    expect(host.textContent).toContain(START_AT_SIGN_IN_LABEL);
    expect(host.textContent).toContain(START_AT_SIGN_IN_HINT);
    expect(START_AT_SIGN_IN_HINT).toBe('It starts quietly in the tray. Your companion wakes up too.');
    expect(tauri.autostartGet).toHaveBeenCalledTimes(1);
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
  });

  it('is off by default and turning it on saves through the command and reports the OS state', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    vi.mocked(tauri.autostartSet).mockResolvedValue(true);
    const onSaved = vi.fn();
    await mount(onSaved);
    await act(async () => toggle()?.click());
    await flush();
    expect(tauri.autostartSet).toHaveBeenCalledWith(true);
    expect(toggle()?.getAttribute('aria-checked')).toBe('true');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('keeps the switch off and says why when the OS refuses the change', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    vi.mocked(tauri.autostartSet).mockRejectedValue(new Error('registry denied'));
    const onSaved = vi.fn();
    await mount(onSaved);
    await act(async () => toggle()?.click());
    await flush();
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('registry denied');
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('shows the OS state when the command answers something other than the request', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    vi.mocked(tauri.autostartSet).mockResolvedValue(false);
    const onSaved = vi.fn();
    await mount(onSaved);
    await act(async () => toggle()?.click());
    await flush();
    expect(toggle()?.getAttribute('aria-checked')).toBe('false');
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('is disabled when the desktop shell cannot answer', async () => {
    vi.mocked(tauri.autostartGet).mockRejectedValue(new Error('Tauri command unavailable'));
    await mount();
    expect(toggle()?.disabled).toBe(true);
    expect(tauri.autostartSet).not.toHaveBeenCalled();
  });
});
