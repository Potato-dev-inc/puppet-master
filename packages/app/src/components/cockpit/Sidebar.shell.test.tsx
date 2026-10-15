// @vitest-environment jsdom
// The shell's sidebar: no raw ids, no bridge address, and the "+" menu's Separate copy default.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { GitState, ShipApi } from '../../lib/ship-api';
import { tauri } from '../../lib/tauri';
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
  localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const COPY_CWD = 'C:\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c';

function fakeApi(over: Record<string, unknown> = {}, cwd = 'C:\\work\\shop') {
  const info = { id: 'p1', agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd, cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  const panes = [{ info, status: 'idle' }];
  return {
    registry: { paneList: panes, panes: new Map([['p1', panes[0]]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    asked: { questions: [] },
    launchEntries: [{ type: 'codex', label: 'Codex' }],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
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
    bridgeLine: '',
    ...over,
  } as unknown as CockpitApi;
}

const ship = (state: GitState | Error): ShipApi =>
  ({
    recent: vi.fn().mockResolvedValue([]),
    gitState: vi.fn(() => (state instanceof Error ? Promise.reject(state) : Promise.resolve(state))),
    gitInitPlan: vi.fn().mockResolvedValue({ path: 'C:\\work\\shop', name: 'shop', state: 'folder', commands: ['git init'], files: 3, too_big: false, gitignore: [] }),
    gitInit: vi.fn(),
    forget: vi.fn(),
    pickFolder: vi.fn(),
  }) as unknown as ShipApi;

async function render(c: CockpitApi, s: ShipApi = ship('repo')) {
  await act(async () => root.render(<Sidebar c={c} onOpenPalette={() => {}} ship={s} />));
  await act(async () => {});
}

async function openMenu(): Promise<HTMLElement> {
  const plus = host.querySelector<HTMLButtonElement>('button[aria-label="New agent"]')!;
  await act(async () => plus.click());
  await act(async () => {});
  return host.querySelector<HTMLElement>('[role="menu"]')!;
}

describe('sidebar: nothing raw', () => {
  it('shows nothing about the bridge while it works, and never its address', async () => {
    await render(fakeApi({ bridgeLine: '' }));
    expect(host.querySelector('.ck-bridge')).toBeNull();
    expect(host.textContent).not.toMatch(/127\.0\.0\.1|localhost|Bridge/);
  });

  it('says "Not connected" with a Fix button that opens the bridge settings, when it is down', async () => {
    const c = fakeApi({ bridgeLine: 'Not connected' });
    await render(c);
    const line = host.querySelector('.ck-bridge')!;
    expect(line.textContent).toContain('Not connected');
    act(() => line.querySelector<HTMLButtonElement>('button')!.click());
    expect(c.openSettings).toHaveBeenCalledWith('bridge');
  });

  it('names a pane in a separate copy by its project, never by the id of the copy folder', async () => {
    await render(fakeApi({}, COPY_CWD));
    const row = host.querySelector('.ck-row:not(.ck-row--flat)')!;
    expect(row.querySelector('.ck-row-sub')!.textContent).toBe('shop · separate copy');
    expect(row.textContent).not.toContain('7d4cb2b3');
    expect(row.querySelector('.ck-row-main')!.getAttribute('title')).not.toContain('7d4cb2b3');
  });

  it('calls the Project view "Project", not "Project memory"', async () => {
    await act(async () => root.render(<Sidebar c={fakeApi()} onOpenPalette={() => {}} onOpenMemory={() => {}} ship={ship('repo')} />));
    const labels = [...host.querySelectorAll('.ck-side-foot button')].map((b) => b.textContent?.trim());
    expect(labels).toContain('Project');
    expect(labels).not.toContain('Project memory');
  });

  it('gives the agents\' questions a row, so the caption count matches what is listed', async () => {
    const q = { id: 'q1', question: 'Which branch?', choices: [], allow_free_text: true, urgency: 'normal', created_at_ms: 1, expires_at_ms: 9e12, state: 'pending' };
    const c = fakeApi({ asked: { questions: [q] } });
    await render(c);
    const group = [...host.querySelectorAll('.ck-group')].find((g) => g.querySelector('.ck-caption')?.textContent?.startsWith('Needs you'))!;
    expect(group.querySelector('.ck-caption-count')!.textContent).toBe('1');
    const row = [...group.querySelectorAll('.ck-row')].find((r) => r.textContent?.includes('Questions from agents'))!;
    expect(row.querySelector('.ck-row-time')!.textContent).toBe('1');
    act(() => (row as HTMLButtonElement).click());
    expect(c.select).toHaveBeenCalledWith(null);
  });
});

describe('"+" menu: Separate copy default', () => {
  const sw = (menu: HTMLElement) => menu.querySelector<HTMLButtonElement>('[role="switch"]')!;

  it('starts ON for a Git project with a first commit', async () => {
    await render(fakeApi(), ship('repo'));
    const menu = await openMenu();
    expect(sw(menu).getAttribute('aria-checked')).toBe('true');
    expect(sw(menu).disabled).toBe(false);
    expect(menu.textContent).toContain('uncommitted changes');
  });

  it('starts a new agent in a separate copy when it is on', async () => {
    const c = fakeApi();
    await render(c, ship('repo'));
    const menu = await openMenu();
    const codex = [...menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((b) => b.textContent?.includes('Codex'))!;
    await act(async () => codex.click());
    expect(c.newAgent).toHaveBeenCalledWith('codex', undefined, true, 'C:\\work\\shop');
  });

  it('the remembered choice wins: a project switched off stays off', async () => {
    localStorage.setItem('pm.separate-copy', JSON.stringify({ 'C:\\work\\shop': false }));
    await render(fakeApi(), ship('repo'));
    const menu = await openMenu();
    expect(sw(menu).getAttribute('aria-checked')).toBe('false');
  });

  it('remembers a change for the next time', async () => {
    await render(fakeApi(), ship('repo'));
    const menu = await openMenu();
    await act(async () => sw(menu).click());
    expect(sw(menu).getAttribute('aria-checked')).toBe('false');
    expect(JSON.parse(localStorage.getItem('pm.separate-copy')!)).toEqual({ 'C:\\work\\shop': false });
  });

  it('is OFF without Git, with a one-line hint and "Set up Git here"', async () => {
    await render(fakeApi(), ship('folder'));
    const menu = await openMenu();
    expect(sw(menu).getAttribute('aria-checked')).toBe('false');
    expect(sw(menu).disabled).toBe(true);
    expect(menu.textContent).toContain('Needs Git');
    const setUp = [...menu.querySelectorAll('button')].find((b) => b.textContent === 'Set up Git here')!;
    expect(setUp).toBeTruthy();
    await act(async () => setUp.click());
    await act(async () => {});
    expect(menu.textContent).toContain("This folder doesn't use Git yet");
  });

  it('is OFF for a folder with no first snapshot', async () => {
    await render(fakeApi(), ship('no_commits'));
    const menu = await openMenu();
    expect(sw(menu).getAttribute('aria-checked')).toBe('false');
    expect(menu.textContent).toContain('first Git snapshot');
  });

  it('stays off, and usable, when the Git state cannot be read', async () => {
    await render(fakeApi(), ship(new Error('Desktop shell required')));
    const menu = await openMenu();
    expect(sw(menu).getAttribute('aria-checked')).toBe('false');
    expect(sw(menu).disabled).toBe(false);
  });

  it('with no pane selected, it names the open project and starts on for a Git project', async () => {
    vi.spyOn(tauri, 'getProjectPath').mockResolvedValue('C:\\work\\shop');
    const s = ship('repo');
    await render(fakeApi({ selectedId: null }), s);
    const menu = await openMenu();
    expect(menu.querySelector('.ck-chip-name')!.textContent).toBe('shop');
    expect(sw(menu).getAttribute('aria-checked')).toBe('true');
    expect(s.gitState).toHaveBeenCalledWith('C:\\work\\shop');
  });

  it('a pane in a separate copy works out its project from the copy folder', async () => {
    const s = ship('repo');
    await render(fakeApi({}, COPY_CWD), s);
    expect(s.gitState).toHaveBeenCalledWith('C:\\work\\shop');
  });
});
