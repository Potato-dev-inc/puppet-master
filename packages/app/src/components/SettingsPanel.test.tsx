// @vitest-environment jsdom
// The Settings shell: sections, row search, Coming later, autosave plumbing, fresh state, confirm sheet.
// The catalog is replaced by a tiny one so these tests cover the panel, not the sections (each section has its own test).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Settings } from '@puppet-master/shared';
import { BASE_SETTINGS, blur, click, settle, type as typeInto } from './settings/test-utils';
import type { SettingsRefreshInfo } from '../lib/settings-sync';

vi.mock('./settings/catalog', async () => {
  const { SettingRow, SettingToggle, Btn } = await import('./settings/settings-ui');
  const { NumberControl, TextControl } = await import('./settings/controls');
  return {
    LATER_ITEMS: [
      { title: 'Cost controls', note: 'Daily limits.' },
      { title: 'Plugins', note: 'Add-ons.' },
    ],
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
            group: 'Look',
            render: (env: import('./settings/model').SettingsEnv) => (
              <SettingToggle label="Dark theme" description="Dark or light." checked={env.settings.theme === 'dark'} onChange={(on) => void env.patch({ theme: on ? 'dark' : 'light' })} />
            ),
          },
          {
            id: 'away',
            label: 'Away digest after',
            hint: 'Minutes.',
            group: 'Look',
            render: (env: import('./settings/model').SettingsEnv) => (
              <SettingRow label="Away digest after">
                <NumberControl aria-label="Away" min={0} max={1440} value={env.settings.away_minutes ?? 5} onCommit={(n) => env.patch({ away_minutes: n })} />
              </SettingRow>
            ),
          },
          {
            id: 'folder',
            label: 'Default project folder',
            hint: 'Where panes start.',
            keywords: 'workspace',
            render: (env: import('./settings/model').SettingsEnv) => (
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
            render: (env: import('./settings/model').SettingsEnv) => (
              <SettingRow label="Erase everything">
                <Btn
                  onClick={async () => {
                    const ok = await env.confirm({ title: 'Erase everything?', body: 'Cannot be undone.', confirmLabel: 'Erase' });
                    if (ok) await env.patch({ project_path: null });
                  }}
                >
                  Erase…
                </Btn>
              </SettingRow>
            ),
          },
        ],
      },
      {
        id: 'keys',
        title: 'Keys',
        blurb: 'Keys.',
        rows: [
          {
            id: 'key-anthropic',
            label: 'Anthropic API key',
            hint: 'Stored in the keychain.',
            render: () => <SettingRow label="Anthropic API key" />,
          },
        ],
      },
    ],
  };
});

import { SettingsPanel, type SettingsApi } from './SettingsPanel';

let host: HTMLDivElement;
let root: Root;
let current: Settings;
let refresh: ((info: SettingsRefreshInfo) => void) | null;
let api: SettingsApi & { load: ReturnType<typeof vi.fn>; patch: ReturnType<typeof vi.fn> };

function makeApi(): typeof api {
  return {
    load: vi.fn(async () => ({ ...current })),
    patch: vi.fn(async (patch: Record<string, unknown>) => {
      for (const [k, v] of Object.entries(patch)) {
        if (v === null || v === undefined) delete (current as unknown as Record<string, unknown>)[k];
        else (current as unknown as Record<string, unknown>)[k] = v;
      }
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
}

async function mount(props: Partial<React.ComponentProps<typeof SettingsPanel>> = {}) {
  await act(async () => {
    root.render(<SettingsPanel open api={api} {...props} />);
  });
  await settle(5);
}

const main = () => host.querySelector('.st-main') as HTMLElement;
const search = () => host.querySelector('input[aria-label="Search settings"]') as HTMLInputElement;
const nav = (title: string) => [...host.querySelectorAll('.st-nav-row')].find((b) => b.textContent?.includes(title)) as HTMLButtonElement;
const sw = (label: string) => host.querySelector(`[role="switch"][aria-label="${label}"]`) as HTMLButtonElement;

beforeEach(() => {
  current = { ...BASE_SETTINGS };
  refresh = null;
  api = makeApi();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('shell', () => {
  it('lists the sections in the sidebar, opens on the first, and has one quiet Coming later row', async () => {
    await mount();
    expect([...host.querySelectorAll('.st-nav .st-nav-row')].map((b) => b.textContent)).toEqual(['General', 'Keys']);
    expect(nav('General').getAttribute('aria-current')).toBe('page');
    expect(main().querySelector('h1')!.textContent).toBe('General');
    expect(host.querySelector('.st-side-foot')!.textContent).toBe('Coming later');
  });

  it('has no Save, Done or Saved footer and no Planned badge anywhere', async () => {
    await mount();
    const buttons = [...host.querySelectorAll('button')].map((b) => b.textContent);
    for (const gone of ['Save changes', 'Save', 'Done']) expect(buttons).not.toContain(gone);
    expect(host.textContent).not.toContain('Planned');
    expect(host.textContent).not.toContain('Not implemented');
    expect(host.querySelector('.st-saved')!.textContent).toBe(''); // the tick is empty until something saves
  });

  it('opens on the requested page and switches pages from the sidebar', async () => {
    await mount({ initialTab: 'keys' });
    expect(main().querySelector('h1')!.textContent).toBe('Keys');
    await click(nav('General'));
    expect(main().querySelector('h1')!.textContent).toBe('General');
    expect(main().textContent).toContain('Dark theme');
  });

  it('renders rows grouped: a heading per group, destructive rows in their own last list', async () => {
    await mount();
    const groups = [...main().querySelectorAll('section.st-group')];
    expect(groups.map((g) => g.querySelector('h2')?.textContent ?? '')).toEqual(['Look', '', '']);
    expect(groups[0].textContent).toContain('Dark theme');
    expect(groups[1].textContent).toContain('Default project folder');
    expect(groups[2].getAttribute('data-danger')).toBe('true');
    expect(groups[2].textContent).toContain('Erase everything');
  });

  it('shows the Back button only when it can close', async () => {
    await mount();
    expect(host.querySelector('[aria-label="Close settings"]')).toBeNull();
    const onClose = vi.fn();
    await mount({ onClose });
    await click(host.querySelector('[aria-label="Close settings"]'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('renders nothing when closed', async () => {
    await mount({ open: false });
    expect(host.querySelector('.settings')).toBeNull();
  });
});

describe('search', () => {
  it('filters rows (not just section names) by label, hint and keywords, grouped under their section', async () => {
    await mount();
    await typeInto(search(), 'keychain');
    expect(main().querySelector('h1')!.textContent).toBe('Search results');
    const sections = [...main().querySelectorAll('.st-results > section')];
    expect(sections.map((s) => s.querySelector('h2')!.textContent)).toEqual(['Keys']);
    expect(sections[0].textContent).toContain('Anthropic API key');
    expect(main().textContent).not.toContain('Dark theme');

    await typeInto(search(), 'workspace'); // a keyword, not in the label or hint
    expect([...main().querySelectorAll('.st-results > section h2')].map((h) => h.textContent)).toEqual(['General']);
    expect(main().textContent).toContain('Default project folder');

    await typeInto(search(), 'minutes'); // a hint
    expect(main().textContent).toContain('Away digest after');
    expect(main().textContent).not.toContain('Default project folder');
  });

  it('requires every word to match, ignores case, and says so when nothing matches', async () => {
    await mount();
    await typeInto(search(), 'DARK light');
    expect(main().textContent).toContain('Dark theme');
    await typeInto(search(), 'dark keychain');
    expect(main().textContent).toContain('No settings match');
  });

  it('search rows are live controls, and Escape or picking a section leaves search', async () => {
    await mount();
    await typeInto(search(), 'theme');
    await click(sw('Dark theme'));
    expect(api.patch).toHaveBeenCalledWith({ theme: 'light' });
    await act(async () => {
      search().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(search().value).toBe('');
    expect(main().querySelector('h1')!.textContent).toBe('General');
    await typeInto(search(), 'keychain');
    await click(nav('General'));
    expect(search().value).toBe('');
    expect(main().querySelector('h1')!.textContent).toBe('General');
  });

  it('a destructive row found by search still sits in its own list', async () => {
    await mount();
    await typeInto(search(), 'erase');
    const group = main().querySelector('section.st-group')!;
    expect(group.getAttribute('data-danger')).toBe('true');
  });
});

describe('Coming later', () => {
  it('is a plain read-only list: no inputs, switches or buttons, honest copy, no Planned badge', async () => {
    await mount();
    await click(nav('Coming later'));
    expect(main().querySelector('h1')!.textContent).toBe('Coming later');
    expect(main().textContent).toContain('Not built yet');
    expect(main().textContent).toContain('Cost controls');
    expect(main().textContent).toContain('Plugins');
    expect(main().querySelectorAll('input, select, button, [role="switch"]')).toHaveLength(0);
    expect([...main().querySelectorAll('.st-value')].every((v) => v.textContent === 'Not available yet')).toBe(true);
    expect(main().textContent).not.toContain('Planned');
    expect(nav('Coming later').getAttribute('aria-current')).toBe('page');
  });
});

describe('autosave through partial patches', () => {
  it('a switch saves immediately with only its own key and flashes a Saved tick that fades', async () => {
    vi.useFakeTimers();
    await mount();
    await click(sw('Dark theme'));
    await settle();
    expect(api.patch).toHaveBeenCalledExactlyOnceWith({ theme: 'light' });
    const tick = host.querySelector('[data-row="theme"] .st-saved')!;
    expect(tick.textContent).toBe('Saved');
    expect(tick.classList.contains('is-on')).toBe(true);
    await act(async () => void (await vi.advanceTimersByTimeAsync(2000)));
    expect(host.querySelector('[data-row="theme"] .st-saved')!.textContent).toBe('');
  });

  it('text saves after a pause with the one key, number saves on blur clamped', async () => {
    vi.useFakeTimers();
    await mount();
    await typeInto(host.querySelector('input[aria-label="Folder"]')!, 'C:\\work');
    expect(api.patch).not.toHaveBeenCalled();
    await act(async () => void (await vi.advanceTimersByTimeAsync(700)));
    expect(api.patch).toHaveBeenCalledExactlyOnceWith({ project_path: 'C:\\work' });
    const away = host.querySelector('input[aria-label="Away"]') as HTMLInputElement;
    await typeInto(away, '99999');
    await blur(away);
    expect(api.patch).toHaveBeenLastCalledWith({ away_minutes: 1440 });
  });

  it('two controls never send each other\'s keys (a stale snapshot cannot be written)', async () => {
    await mount();
    await click(sw('Dark theme'));
    const away = host.querySelector('input[aria-label="Away"]') as HTMLInputElement;
    await typeInto(away, '12');
    await blur(away);
    const sent = api.patch.mock.calls.map((c) => Object.keys(c[0] as object));
    expect(sent).toEqual([['theme'], ['away_minutes']]);
  });

  it('a failed save shows a plain error on that row and puts the real value back', async () => {
    await mount();
    api.patch.mockRejectedValueOnce(new Error('disk is full'));
    await click(sw('Dark theme'));
    await settle(6);
    expect(host.querySelector('[data-row="theme"] [role="alert"]')!.textContent).toBe('Not saved: disk is full');
    expect(sw('Dark theme').getAttribute('aria-checked')).toBe('true'); // reloaded from the saved state
  });

  it('shows the optimistic value at once and keeps it while a save is in flight', async () => {
    await mount();
    let release: () => void = () => undefined;
    api.patch.mockImplementationOnce(async () => {
      await new Promise<void>((r) => (release = r));
      return { ...current };
    });
    await click(sw('Dark theme'));
    expect(sw('Dark theme').getAttribute('aria-checked')).toBe('false');
    await act(async () => refresh!({ reason: 'changed' })); // a change event arrives mid-save
    await settle();
    expect(sw('Dark theme').getAttribute('aria-checked')).toBe('false'); // not flickered back by a stale reload
    await act(async () => release());
    await settle(5);
    expect(api.load.mock.calls.length).toBeGreaterThanOrEqual(2); // reloaded once the save landed
  });

  it('calls onSaved after a save', async () => {
    const onSaved = vi.fn();
    await mount({ onSaved });
    await click(sw('Dark theme'));
    await settle();
    expect(onSaved).toHaveBeenCalledTimes(1);
  });
});

describe('fresh state', () => {
  it('loads when it opens and subscribes to changes', async () => {
    await mount();
    expect(api.load).toHaveBeenCalledTimes(1);
    expect(api.subscribe).toHaveBeenCalledTimes(1);
    expect(sw('Dark theme').getAttribute('aria-checked')).toBe('true');
  });

  it('reloads when settings change in another window', async () => {
    await mount();
    current.theme = 'light'; // another window saved
    await act(async () => refresh!({ reason: 'changed' }));
    await settle();
    expect(api.load).toHaveBeenCalledTimes(2);
    expect(sw('Dark theme').getAttribute('aria-checked')).toBe('false');
  });

  it('reloads when the hidden window is shown again, and goes to the tab it was asked for', async () => {
    await mount();
    current.theme = 'light';
    await act(async () => refresh!({ reason: 'show', tab: 'nope' }));
    await settle();
    expect(api.load).toHaveBeenCalledTimes(2);
    // An unknown tab: stay put, but still refresh.
    expect(main().querySelector('h1')!.textContent).toBe('General');
    await act(async () => refresh!({ reason: 'show', tab: 'keys' }));
    await settle();
    expect(main().querySelector('h1')!.textContent).toBe('Keys');
    await act(async () => refresh!({ reason: 'show', tab: 'later' }));
    await settle();
    expect(main().querySelector('h1')!.textContent).toBe('Coming later');
  });

  it('reloads when the window regains focus', async () => {
    await mount();
    await act(async () => refresh!({ reason: 'focus' }));
    await settle();
    expect(api.load).toHaveBeenCalledTimes(2);
  });

  it('unsubscribes when it closes', async () => {
    await mount();
    expect(refresh).not.toBeNull();
    await act(async () => root.render(<SettingsPanel open={false} api={api} />));
    expect(refresh).toBeNull();
  });
});

describe('destructive actions use the confirm sheet', () => {
  const eraseButton = () => [...host.querySelectorAll('button')].find((b) => b.textContent === 'Erase…') as HTMLButtonElement;

  it('asks first; Cancel does nothing and has focus', async () => {
    await mount();
    await click(eraseButton());
    const dialog = host.querySelector('[role="dialog"][aria-modal="true"]') as HTMLElement;
    expect(dialog.textContent).toContain('Erase everything?');
    expect(document.activeElement?.textContent).toBe('Cancel');
    await click([...dialog.querySelectorAll('button')].find((b) => b.textContent === 'Cancel'));
    await settle();
    expect(host.querySelector('[aria-modal="true"]')).toBeNull();
    expect(api.patch).not.toHaveBeenCalled();
  });

  it('runs the action only after the person confirms', async () => {
    await mount();
    await click(eraseButton());
    await click([...host.querySelectorAll('.st-sheet button')].find((b) => b.textContent === 'Erase'));
    await settle();
    expect(api.patch).toHaveBeenCalledExactlyOnceWith({ project_path: null });
    expect(host.querySelector('[aria-modal="true"]')).toBeNull();
  });
});
