// @vitest-environment jsdom
// The standalone Settings window (Individual mode). When the mode switches to Orchestration while it is open,
// Rust asks it to move into the cockpit: it names its current page to `open_settings` (which shows Settings
// inline and hides this window), then closes itself. Nothing happens if the cockpit could not take it.
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  toInline: null as null | (() => void),
  openSettings: null as unknown as ReturnType<typeof import('vitest').vi.fn>,
  close: null as unknown as ReturnType<typeof import('vitest').vi.fn>,
  page: 'agents' as string,
}));

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ close: h.close }) }));
vi.mock('./lib/cockpit-api', () => ({
  cockpitApi: {
    onSettingsToInline: (cb: () => void) => {
      h.toInline = cb;
      return Promise.resolve(() => {
        h.toInline = null;
      });
    },
    openSettings: (tab?: string) => h.openSettings(tab),
  },
}));
vi.mock('./hooks/useProjectPath', () => ({
  useProjectPath: () => ({ projectPath: null, setProjectPath: async () => {}, ready: true }),
}));
vi.mock('./hooks/useAppUpdateCheck', () => ({
  useAppUpdateCheck: () => ({ checking: false, result: null, dismissed: false, dismiss: () => {}, refresh: async () => {}, openRelease: async () => {} }),
}));
vi.mock('./components/UpdateAvailableBanner', () => ({ UpdateAvailableBanner: () => null }));
// The panel is stubbed: this test is about the window, not the panel.
vi.mock('./components/SettingsPanel', async () => {
  const { createElement: e, useEffect } = await import('react');
  type Props = { initialTab: string; onPageChange?: (p: string) => void; onClose?: () => void };
  return {
    SettingsPanel: ({ initialTab, onPageChange, onClose }: Props) => {
      useEffect(() => onPageChange?.(initialTab), []); // eslint-disable-line react-hooks/exhaustive-deps
      return e(
        'div',
        null,
        e('span', null, 'PANEL ' + initialTab),
        e('button', { type: 'button', onClick: () => onPageChange?.(h.page) }, 'go'),
        e('button', { type: 'button', onClick: onClose }, 'close'),
      );
    },
  };
});

import SettingsApp from './SettingsApp';
import { requestBringChats } from './lib/chatImport';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

async function mount(search = '?settings=1&tab=agents') {
  window.history.replaceState(null, '', `/${search}`);
  await act(async () => root.render(createElement(SettingsApp)));
}
const click = (text: string) => act(() => [...host.querySelectorAll('button')].find((b) => b.textContent === text)!.click());

beforeEach(() => {
  h.toInline = null;
  h.openSettings = vi.fn(async () => {});
  h.close = vi.fn(async () => {});
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe('Settings window', () => {
  it('opens on the page in its URL and its close button closes only the window', async () => {
    await mount('?settings=1&tab=keys');
    expect(host.textContent).toContain('PANEL keys');
    await click('close');
    expect(h.close).toHaveBeenCalledTimes(1);
    expect(h.openSettings).not.toHaveBeenCalled();
  });

  it('moves into the cockpit when asked, on the page it shows, then closes', async () => {
    await mount('?settings=1&tab=agents');
    h.page = 'phone';
    await click('go'); // the person went to Phone
    expect(h.toInline).not.toBeNull();
    await act(async () => {
      h.toInline!();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(h.openSettings).toHaveBeenCalledWith('phone');
    expect(h.close).toHaveBeenCalledTimes(1);
  });

  it('stays open if the cockpit could not take Settings (it is never lost)', async () => {
    h.openSettings = vi.fn(async () => {
      throw new Error('no cockpit');
    });
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    await mount();
    await act(async () => {
      h.toInline!();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(h.close).not.toHaveBeenCalled();
    expect(logged).toHaveBeenCalledWith('open_settings failed', expect.any(Error));
  });

  it('shows the Bring-chats panel when Settings asks for it (this window has no cockpit)', async () => {
    await mount();
    expect(host.querySelector('.bc-panel')).toBeNull();
    await act(async () => requestBringChats());
    expect(host.querySelector('[data-testid="bring-chats-standalone"] .bc-panel')).not.toBeNull();
    await act(async () => {
      [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Close')!.click();
    });
    expect(host.querySelector('.bc-panel')).toBeNull();
  });

  it('stops listening when it unmounts', async () => {
    await mount();
    expect(h.toInline).not.toBeNull();
    act(() => root.unmount());
    root = createRoot(host);
    expect(h.toInline).toBeNull();
  });
});
