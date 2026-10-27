// @vitest-environment jsdom
// The sidebar's companion rows: above Inbox, the open one marked, the one quiet "Make" row, and the companion's pane
// never listed a second time in the Needs you / Working / Idle groups.
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { CompanionView } from '../../lib/companion';
import type { GitState, ShipApi } from '../../lib/ship-api';
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

const CWD = 'C:\\work\\shop';

function pane(id: string, status: 'idle' | 'running' | 'waiting_input' = 'idle') {
  const info = { id, agent_type: 'codex', pid: 1, status, created_at: 1, last_output_at: null, cwd: CWD, cols: 80, rows: 24, title: null } as unknown as PaneInfo;
  return { info, status };
}

function fakeApi(panes: ReturnType<typeof pane>[]): CockpitApi {
  return {
    registry: { paneList: panes, panes: new Map(panes.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([
      ['p1', 'Codex'],
      ['companion-mochi', 'Mochi'],
    ]),
    asked: { questions: [] },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map(),
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
  } as unknown as CockpitApi;
}

// A fresh fake per render: `vi.restoreAllMocks` in afterEach would drop a shared one's resolved values.
const makeShip = () =>
  ({
    recent: vi.fn().mockResolvedValue([]),
    gitState: vi.fn(() => Promise.resolve('repo' as GitState)),
    gitInitPlan: vi.fn(),
    gitInit: vi.fn(),
    forget: vi.fn(),
    pickFolder: vi.fn(),
  }) as unknown as ShipApi;

const companion = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'codex',
  agentLabel: 'Codex',
  folderName: 'home',
  state: 'working',
  stateLine: 'Working…',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

async function render(props: Partial<ComponentProps<typeof Sidebar>>, panes = [pane('p1'), pane('companion-mochi', 'running')]) {
  await act(async () => root.render(<Sidebar c={fakeApi(panes)} onOpenPalette={() => {}} ship={makeShip()} {...props} />));
  await act(async () => {});
}

const titles = () => [...host.querySelectorAll('.ck-row-title')].map((t) => t.textContent);

describe('sidebar: companions', () => {
  it('puts the companion rows above Inbox', async () => {
    await render({ companions: [companion()] });
    const rows = [...host.querySelectorAll('.ck-side-scroll > button')];
    expect(rows[0].classList.contains('ck-companion-row')).toBe(true);
    expect(rows[1].textContent).toContain('Inbox');
    expect(rows[0].textContent).toContain('Mochi');
    expect(rows[0].textContent).toContain('Working');
  });

  it("does not list the companion's pane a second time in the groups", async () => {
    await render({ companions: [companion()] });
    // The pane is "Mochi" by name: it may appear once, in its own row above.
    expect(titles().filter((t) => t === 'Mochi')).toHaveLength(1);
    expect(host.querySelectorAll('.ck-companion-row')).toHaveLength(1);
  });

  it('marks the open companion as the active row, and Inbox is not active', async () => {
    await render({ companions: [companion()], openCompanionId: 'mochi' });
    expect(host.querySelector('.ck-companion-row')?.getAttribute('aria-current')).toBe('true');
    expect(host.querySelector('.ck-row--flat')?.classList.contains('is-active')).toBe(false);
  });

  it('opens a companion when its row is clicked', async () => {
    const onOpenCompanion = vi.fn();
    await render({ companions: [companion()], onOpenCompanion });
    act(() => host.querySelector<HTMLButtonElement>('.ck-companion-row')!.click());
    expect(onOpenCompanion).toHaveBeenCalledWith('mochi');
  });

  it('with no companion, shows one quiet "Make a companion" row', async () => {
    const onNewCompanion = vi.fn();
    await render({ companions: [], onNewCompanion });
    const row = host.querySelector<HTMLButtonElement>('.ck-companion-row--empty')!;
    expect(row.textContent).toContain('Make a companion');
    act(() => row.click());
    expect(onNewCompanion).toHaveBeenCalledTimes(1);
  });

  it('with no companion list at all (no backend), shows no companion rows', async () => {
    await render({ companions: null });
    expect(host.querySelector('.ck-companion-row')).toBeNull();
    expect(host.textContent).not.toContain('Make a companion');
  });
});
