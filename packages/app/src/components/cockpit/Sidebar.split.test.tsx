// @vitest-environment jsdom
// The pane row's right-click menu: "Open to the side" (split view). Shown only when the shell gives a handler.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
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

function fakeApi(): CockpitApi {
  const mk = (id: string) => {
    const info = { id, agent_type: 'codex', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title: null } as unknown as PaneInfo;
    return { info, status: 'idle' };
  };
  const panes = [mk('p1'), mk('p2')];
  return {
    registry: { paneList: panes, panes: new Map(panes.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([
      ['p1', 'Codex'],
      ['p2', 'Codex two'],
    ]),
    asked: { questions: [] },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: 'p1',
    select: vi.fn(),
    prompts: {},
    statusSince: new Map([
      ['p1', 0],
      ['p2', 0],
    ]),
    now: 60_000,
    spawn: null,
    toasts: [],
    notify: vi.fn(),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

// Made per test: vi.restoreAllMocks in afterEach would empty a shared mock's implementation.
function makeShip(): ShipApi {
  return {
    recent: vi.fn(async () => []),
    gitState: vi.fn(async () => 'repo' as unknown as GitState),
    gitInitPlan: vi.fn(),
    gitInit: vi.fn(),
    forget: vi.fn(),
    pickFolder: vi.fn(),
  } as unknown as ShipApi;
}

async function render(c: CockpitApi, onOpenToSide?: (id: string) => void) {
  await act(async () => root.render(<Sidebar c={c} onOpenPalette={() => {}} ship={makeShip()} onOpenToSide={onOpenToSide} />));
  await act(async () => {});
}

/** The pane row whose title is the given name ("Codex" is p1, "Codex two" is p2). */
function rowOf(name: string): HTMLButtonElement {
  const row = [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.querySelector('.ck-row-title')?.textContent === name);
  if (!row) throw new Error(`no row named ${name}`);
  return row;
}

function rightClick(el: HTMLElement) {
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 50 }));
}

describe('sidebar: Open to the side', () => {
  it('a right-click on a pane row opens a menu with "Open to the side", and that item calls the handler with the row', async () => {
    const onOpenToSide = vi.fn();
    await render(fakeApi(), onOpenToSide);
    await act(async () => rightClick(rowOf('Codex two')));
    const item = [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((b) => b.textContent === 'Open to the side');
    expect(item).toBeTruthy();
    await act(async () => item!.click());
    expect(onOpenToSide).toHaveBeenCalledWith('p2');
    expect(host.querySelector('[role="menuitem"]')).toBeNull();
  });

  it('without a handler the row has no menu at all', async () => {
    await render(fakeApi());
    await act(async () => rightClick(rowOf('Codex two')));
    expect(host.querySelector('[role="menuitem"]')).toBeNull();
  });
});
