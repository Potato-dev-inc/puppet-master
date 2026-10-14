// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import { cockpitApi } from '../../lib/cockpit-api';
import { Palette } from './Palette';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView ??= () => {};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

/** A minimal CockpitApi: only what Sidebar and Palette read. One pane so the list is not empty. */
function fakeApi(overrides: Partial<Record<'openSettings' | 'notify', unknown>> = {}) {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\work\proj', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map([['p1', 0]]),
    now: 60_000,
    spawn: null,
    dismissSpawn: vi.fn(),
    toasts: [],
    notify: vi.fn(),
    setMode: vi.fn(async () => {}),
    toggleTheme: vi.fn(async () => {}),
    newAgent: vi.fn(async () => {}),
    interrupt: vi.fn(async () => {}),
    openSettings: vi.fn(async () => {}),
    bridgeLine: 'Bridge test',
    ...overrides,
  } as unknown as CockpitApi & { openSettings: ReturnType<typeof vi.fn>; notify: ReturnType<typeof vi.fn> };
}

function settingsButton(): HTMLButtonElement {
  const btn = Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === 'Settings');
  if (!btn) throw new Error('Settings button not rendered');
  return btn;
}

/**
 * jsdom does not turn a keydown into a click the way a browser does for a native <button>
 * (Enter clicks on keydown, Space on keyup). Mirror that default action unless keydown was prevented.
 */
function pressKeyOnButton(btn: HTMLElement, key: 'Enter' | ' ') {
  btn.focus();
  expect(document.activeElement).toBe(btn);
  const down = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
  act(() => {
    btn.dispatchEvent(down);
  });
  if (down.defaultPrevented) return;
  if (key === ' ') {
    act(() => {
      btn.dispatchEvent(new KeyboardEvent('keyup', { key, bubbles: true, cancelable: true }));
    });
  }
  act(() => btn.click());
}

describe('Sidebar Settings button', () => {
  it('is a native button, so the keyboard reaches it', () => {
    act(() => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} />));
    expect(settingsButton().tagName).toBe('BUTTON');
    expect(settingsButton().type).toBe('button');
  });

  it('opens settings on element.click()', () => {
    const api = fakeApi();
    act(() => root.render(<Sidebar c={api} onOpenPalette={() => {}} />));
    act(() => settingsButton().click());
    expect(api.openSettings).toHaveBeenCalledTimes(1);
  });

  it('opens settings on a bubbling click event (what fireEvent.click dispatches)', () => {
    const api = fakeApi();
    act(() => root.render(<Sidebar c={api} onOpenPalette={() => {}} />));
    act(() => {
      settingsButton().dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    });
    expect(api.openSettings).toHaveBeenCalledTimes(1);
  });

  it('opens settings on Enter', () => {
    const api = fakeApi();
    act(() => root.render(<Sidebar c={api} onOpenPalette={() => {}} />));
    pressKeyOnButton(settingsButton(), 'Enter');
    expect(api.openSettings).toHaveBeenCalledTimes(1);
  });

  it('opens settings on Space', () => {
    const api = fakeApi();
    act(() => root.render(<Sidebar c={api} onOpenPalette={() => {}} />));
    pressKeyOnButton(settingsButton(), ' ');
    expect(api.openSettings).toHaveBeenCalledTimes(1);
  });
});

describe('open_settings failures are surfaced, not swallowed', () => {
  it('the Palette "Toolbelt" action reports a failed open_settings to the user', async () => {
    vi.spyOn(cockpitApi, 'openSettings').mockRejectedValue(new Error('settings window failed'));
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    const api = fakeApi();
    act(() => root.render(<Palette c={api} onClose={vi.fn()} />));
    const row = Array.from(host.querySelectorAll('[role="option"]')).find((r) => r.textContent?.includes('Toolbelt'));
    expect(row).toBeTruthy();
    await act(async () => {
      (row as HTMLElement).click();
    });
    expect(api.notify).toHaveBeenCalledWith(expect.stringContaining('settings window failed'));
    expect(logged).toHaveBeenCalledWith('open_settings failed', expect.any(Error));
  });
});
