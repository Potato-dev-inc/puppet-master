// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
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

/** A minimal CockpitApi: only what Sidebar reads. One pane so the list is not empty. */
function fakeApi() {
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
  } as unknown as CockpitApi;
}

function openNewAgentMenu(): HTMLElement {
  const plus = host.querySelector<HTMLButtonElement>('button[aria-label="New agent"]');
  if (!plus) throw new Error('New agent button not rendered');
  act(() => plus.click());
  const menu = host.querySelector<HTMLElement>('[role="menu"]');
  if (!menu) throw new Error('New agent menu did not open');
  return menu;
}

describe('"Separate copy" switch in the New agent menu', () => {
  it('renders a switch, off by default, with no native checkbox', () => {
    act(() => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} />));
    const menu = openNewAgentMenu();

    const sw = menu.querySelector('[role="switch"]');
    expect(sw).toBeTruthy();
    expect(sw?.getAttribute('aria-checked')).toBe('false');
    expect(menu.textContent).toContain('Separate copy');
    expect(menu.querySelector('input[type="checkbox"]')).toBeNull();
  });

  it('turns on when clicked', () => {
    act(() => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} />));
    const menu = openNewAgentMenu();

    const sw = menu.querySelector<HTMLButtonElement>('[role="switch"]');
    expect(sw).toBeTruthy();
    act(() => sw?.click());
    expect(menu.querySelector('[role="switch"]')?.getAttribute('aria-checked')).toBe('true');
  });
});
