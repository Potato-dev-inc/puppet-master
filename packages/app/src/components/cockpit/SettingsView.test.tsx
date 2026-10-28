// @vitest-environment jsdom
// Settings inside the cockpit: the same SettingsPanel as the Settings window (not a fork), opened inline, with
// Back and Escape as the way out, and fresh state on `settings://changed`.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Settings } from '@puppet-master/shared';
import { BASE_SETTINGS, click, settle } from '../settings/test-utils';
import type { SettingsRefreshInfo } from '../../lib/settings-sync';

vi.mock('../settings/catalog', async () => {
  const { SettingRow, SettingToggle, Btn } = await import('../settings/settings-ui');
  const { TextControl } = await import('../settings/controls');
  type Env = import('../settings/model').SettingsEnv;
  return {
    LATER_ITEMS: [],
    SECTIONS: [
      {
        id: 'general',
        title: 'General',
        blurb: 'General things.',
        rows: [
          {
            id: 'theme',
            label: 'Theme',
            hint: 'Dark or light.',
            render: (env: Env) => (
              <SettingToggle label="Dark theme" description="Dark or light." checked={env.settings.theme === 'dark'} onChange={(on) => void env.patch({ theme: on ? 'dark' : 'light' })} />
            ),
          },
          {
            id: 'folder',
            label: 'Default project folder',
            hint: 'Where panes start.',
            render: (env: Env) => (
              <SettingRow label="Default project folder">
                <TextControl aria-label="Folder" value={env.settings.project_path ?? ''} onCommit={(v) => env.patch({ project_path: v || null })} />
              </SettingRow>
            ),
          },
          {
            id: 'nuke',
            label: 'Erase everything',
            hint: 'Cannot be undone.',
            danger: true,
            render: (env: Env) => (
              <SettingRow label="Erase everything">
                <Btn onClick={() => void env.confirm({ title: 'Erase everything?', body: 'Cannot be undone.', confirmLabel: 'Erase' })}>Erase</Btn>
              </SettingRow>
            ),
          },
        ],
      },
      {
        id: 'agents',
        title: 'Agents',
        blurb: 'Agents.',
        rows: [
          {
            id: 'preload',
            label: 'Preload agents',
            hint: 'Warm agents.',
            render: (env: Env) => <SettingRow label="Preload agents">{env.openStore && <Btn onClick={env.openStore}>Browse more agents</Btn>}</SettingRow>,
          },
        ],
      },
    ],
  };
});
vi.mock('../../hooks/useProjectPath', () => ({
  useProjectPath: () => ({ projectPath: 'C:\\work\\demo', setProjectPath: async () => {}, ready: true }),
}));
vi.mock('../../hooks/useAppUpdateCheck', () => ({
  useAppUpdateCheck: () => ({ checking: false, result: null, dismissed: false, dismiss: () => {}, refresh: async () => {}, openRelease: async () => {} }),
}));

import { SettingsView } from './SettingsView';
import type { SettingsApi } from '../SettingsPanel';

let host: HTMLDivElement;
let root: Root;
let current: Settings;
let refresh: ((info: SettingsRefreshInfo) => void) | null;
let api: SettingsApi & { load: ReturnType<typeof vi.fn>; patch: ReturnType<typeof vi.fn> };
let onBack: ReturnType<typeof vi.fn>;

async function mount(props: Partial<React.ComponentProps<typeof SettingsView>> = {}) {
  await act(async () => {
    root.render(<SettingsView onBack={onBack} api={api} {...props} />);
  });
  await settle(5);
}

const h1 = () => host.querySelector('.st-main h1')!.textContent;
const search = () => host.querySelector('input[aria-label="Search settings"]') as HTMLInputElement;
const folder = () => host.querySelector('input[aria-label="Folder"]') as HTMLInputElement;
function esc(target: EventTarget = document.body) {
  const ev = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
  act(() => {
    target.dispatchEvent(ev);
  });
  return ev;
}

beforeEach(() => {
  current = { ...BASE_SETTINGS };
  refresh = null;
  onBack = vi.fn();
  api = {
    load: vi.fn(async () => ({ ...current })),
    patch: vi.fn(async (patch: Record<string, unknown>) => {
      Object.assign(current, patch);
      return { ...current };
    }),
    findBridgeUrl: vi.fn(async () => 'http://127.0.0.1:17321'),
    subscribe: vi.fn((cb) => {
      refresh = cb;
      return () => {
        refresh = null;
      };
    }),
  } as typeof api;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('inline Settings view', () => {
  it('hands the Agent Store opener to the rows, and only when the host has one', async () => {
    const onOpenStore = vi.fn();
    await mount({ initialTab: 'agents', onOpenStore });
    const browse = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Browse more agents');
    expect(browse).toBeTruthy();
    await act(async () => (browse as HTMLButtonElement).click());
    expect(onOpenStore).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount({ initialTab: 'agents' });
    expect(host.textContent).not.toContain('Browse more agents');
  });

  it('is the Settings panel itself, rendered inline: sections, search, no Save button', async () => {
    await mount();
    expect(host.querySelector('.settings.settings--inline')).not.toBeNull();
    expect([...host.querySelectorAll('.st-nav .st-nav-row')].map((b) => b.textContent)).toEqual(['General', 'Agents']);
    expect(search()).not.toBeNull();
    expect([...host.querySelectorAll('button')].map((b) => b.textContent)).not.toContain('Save');
  });

  it('opens on the page a deep link names (Settings > Agents > Preload)', async () => {
    await mount({ initialTab: 'agents' });
    expect(h1()).toBe('Agents');
    expect(host.textContent).toContain('Preload agents');
  });

  it('autosaves through the same patch path as the window', async () => {
    await mount();
    const sw = host.querySelector('[role="switch"][aria-label="Dark theme"]') as HTMLButtonElement;
    await click(sw);
    expect(api.patch).toHaveBeenCalledWith({ theme: 'light' });
  });

  it('the Back button goes back', async () => {
    await mount();
    const back = host.querySelector('button[aria-label="Back"]') as HTMLButtonElement;
    expect(back).not.toBeNull();
    await click(back);
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});

describe('Escape', () => {
  it('goes back', async () => {
    await mount();
    const ev = esc();
    expect(onBack).toHaveBeenCalledTimes(1);
    expect(ev.defaultPrevented).toBe(true);
  });

  it('goes back from a focused page button too', async () => {
    await mount();
    esc(host.querySelector('.st-nav-row')!);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('first clears a search with text, and does not leave', async () => {
    await mount();
    const input = search();
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, 'theme');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(h1()).toBe('Search results');
    esc(input);
    expect(onBack).not.toHaveBeenCalled();
    expect(h1()).toBe('General');
  });

  it('in a field being typed into only leaves the field; a second Escape goes back', async () => {
    await mount();
    const input = folder();
    input.focus();
    expect(document.activeElement).toBe(input);
    esc(input);
    expect(onBack).not.toHaveBeenCalled();
    expect(document.activeElement).not.toBe(input);
    esc();
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('is left to an open confirm sheet (it closes, Settings stays)', async () => {
    await mount();
    const erase = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Erase') as HTMLButtonElement;
    await click(erase);
    expect(host.querySelector('[role="dialog"]')).not.toBeNull();
    esc(host.querySelector('.st-sheet')!);
    expect(onBack).not.toHaveBeenCalled();
  });

  it('is left to the palette or any scrim that is open', async () => {
    await mount();
    const scrim = document.createElement('div');
    scrim.className = 'ck-scrim';
    document.body.appendChild(scrim);
    esc();
    scrim.remove();
    expect(onBack).not.toHaveBeenCalled();
    esc();
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('is left alone when something already handled it', async () => {
    await mount();
    const handler = (e: Event) => e.preventDefault();
    document.body.addEventListener('keydown', handler);
    esc();
    document.body.removeEventListener('keydown', handler);
    expect(onBack).not.toHaveBeenCalled();
  });

  it('stops listening when the view closes', async () => {
    await mount();
    await act(async () => root.render(<div />));
    esc();
    expect(onBack).not.toHaveBeenCalled();
  });
});

describe('fresh state (settings://changed)', () => {
  it('reloads when the settings change in another window', async () => {
    await mount();
    expect(api.load).toHaveBeenCalledTimes(1);
    expect(api.subscribe).toHaveBeenCalledTimes(1);
    current.theme = 'light';
    await act(async () => refresh!({ reason: 'changed' }));
    await settle(5);
    expect(api.load).toHaveBeenCalledTimes(2);
    const sw = host.querySelector('[role="switch"][aria-label="Dark theme"]') as HTMLButtonElement;
    expect(sw.getAttribute('aria-checked')).toBe('false');
  });

  it('unsubscribes when it goes back', async () => {
    await mount();
    expect(refresh).not.toBeNull();
    await act(async () => root.render(<div />));
    expect(refresh).toBeNull();
  });
});
