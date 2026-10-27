// @vitest-environment jsdom
// The Companion page's Setup card: a one-line hint with "Turn on" while the sign-in start is off. Hidden when it is
// on, and hidden outside the desktop shell. The Tauri commands are mocked; the real plugin is never called.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { StartAtSignInHint } from './StartAtSignInHint';

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

async function flush() {
  for (let i = 0; i < 4; i++) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function mount() {
  await act(async () => {
    root.render(<StartAtSignInHint />);
  });
  await flush();
}

const turnOn = () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Turn on');

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

describe('StartAtSignInHint', () => {
  it('shows one line and a "Turn on" button while the sign-in start is off', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    await mount();
    expect(host.textContent).toContain('After a restart, your companion waits until you open Puppet Master.');
    expect(turnOn()).toBeDefined();
  });

  it('turning it on saves through the command and then hides the hint', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    vi.mocked(tauri.autostartSet).mockResolvedValue(true);
    await mount();
    await act(async () => turnOn()?.click());
    await flush();
    expect(tauri.autostartSet).toHaveBeenCalledWith(true);
    expect(host.textContent).toBe('');
  });

  it('is hidden when the sign-in start is already on', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(true);
    await mount();
    expect(host.textContent).toBe('');
    expect(turnOn()).toBeUndefined();
  });

  it('is hidden outside the desktop shell, where the command cannot answer', async () => {
    vi.mocked(tauri.autostartGet).mockRejectedValue(new Error('Tauri command unavailable'));
    await mount();
    expect(host.textContent).toBe('');
  });

  it('says so when the OS refuses the change and keeps the button', async () => {
    vi.mocked(tauri.autostartGet).mockResolvedValue(false);
    vi.mocked(tauri.autostartSet).mockRejectedValue(new Error('registry denied'));
    await mount();
    await act(async () => turnOn()?.click());
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('registry denied');
    expect(turnOn()).toBeDefined();
  });
});
