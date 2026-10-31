// @vitest-environment jsdom
// The ONE pane menu: a right-click (or the context-menu key) on a pane row in the sidebar. It holds the pane's
// actions (Rename, Reflow, Interrupt, Details, Close; Copy selection is in the command palette) and runs each on the right-clicked pane.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { GitState, ShipApi } from '../../lib/ship-api';
import { subscribePaneActions, takePendingPaneAction } from '../../lib/pane-actions';
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
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  takePendingPaneAction('p1');
  takePendingPaneAction('p2');
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
    interrupt: vi.fn(async () => {}),
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

async function render(c: CockpitApi, extra: { onOpenToSide?: (id: string) => void; onPaneAction?: (id: string, a: string) => void } = {}) {
  await act(async () =>
    root.render(<Sidebar c={c} onOpenPalette={() => {}} ship={makeShip()} onOpenToSide={extra.onOpenToSide} onPaneAction={extra.onPaneAction as never} />),
  );
  await act(async () => {});
}

function rowOf(name: string): HTMLButtonElement {
  const row = [...host.querySelectorAll<HTMLButtonElement>('button.ck-row')].find((b) => b.querySelector('.ck-row-title')?.textContent === name);
  if (!row) throw new Error(`no row named ${name}`);
  return row;
}
const rightClick = (el: HTMLElement, x = 40, y = 50) => el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }));
const items = () => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
const labels = () => items().map((b) => b.textContent!.replace('⌃C', ''));
const item = (label: string) => items().find((b) => b.textContent!.replace('⌃C', '') === label)!;

describe('the pane row menu', () => {
  it('is one compact menu: view item, the pane actions, then Close last, no duplicates, grouped by separators', async () => {
    await render(fakeApi(), { onOpenToSide: vi.fn() });
    await act(async () => rightClick(rowOf('Codex two')));
    expect(labels()).toEqual(['Open to the side', 'Rename', 'Reflow', 'Interrupt', 'Details', 'Close']);
    expect(host.querySelectorAll('[role="menu"]')).toHaveLength(1);
    expect(host.querySelector('[role="menu"]')!.classList.contains('ck-menu')).toBe(true);
    const kids = [...host.querySelector('[role="menu"]')!.children].map((n) => n.getAttribute('role'));
    expect(kids).toEqual(['menuitem', 'separator', 'menuitem', 'menuitem', 'menuitem', 'menuitem', 'separator', 'menuitem']);
  });

  it('runs each action on the right-clicked pane, not the selected one', async () => {
    const c = fakeApi();
    await render(c);
    for (const [label, action] of [
      ['Rename', 'rename'],
      ['Reflow', 'reflow'],
      ['Details', 'details'],
      ['Close', 'close'],
    ] as const) {
      const p2 = vi.fn();
      const p1 = vi.fn();
      const off2 = subscribePaneActions('p2', p2);
      const off1 = subscribePaneActions('p1', p1);
      await act(async () => rightClick(rowOf('Codex two')));
      await act(async () => item(label).click());
      expect(p2).toHaveBeenCalledWith(action);
      expect(p1).not.toHaveBeenCalled();
      expect(c.select).toHaveBeenLastCalledWith('p2');
      expect(host.querySelector('[role="menu"]')).toBeNull();
      off1();
      off2();
    }
  });

  it('Interrupt stops the right-clicked pane', async () => {
    const c = fakeApi();
    await render(c);
    await act(async () => rightClick(rowOf('Codex two')));
    await act(async () => item('Interrupt').click());
    expect(c.interrupt).toHaveBeenCalledWith('p2');
  });

  it('the shell can take over the actions (a split view gives way first)', async () => {
    const onPaneAction = vi.fn();
    const c = fakeApi();
    await render(c, { onPaneAction });
    await act(async () => rightClick(rowOf('Codex two')));
    await act(async () => item('Details').click());
    expect(onPaneAction).toHaveBeenCalledWith('p2', 'details');
    expect(c.select).not.toHaveBeenCalled();
  });

  it('opens from the keyboard (context-menu key, Shift+F10), focuses the first item, arrows move, Escape returns to the row', async () => {
    await render(fakeApi(), { onOpenToSide: vi.fn() });
    const row = rowOf('Codex two');
    row.focus();
    // A keyboard-opened contextmenu has no pointer position.
    await act(async () => rightClick(row, 0, 0));
    expect(document.activeElement).toBe(item('Open to the side'));
    await act(async () => item('Open to the side').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })));
    expect(document.activeElement).toBe(item('Rename'));
    await act(async () => item('Rename').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true })));
    await act(async () => item('Open to the side').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true })));
    expect(document.activeElement).toBe(item('Close'));
    await act(async () => item('Close').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })));
    expect(host.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(row);
  });

  it('is drawn in a portal on the cockpit root, outside the sidebar, and has no Copy selection', async () => {
    await render(fakeApi());
    await act(async () => rightClick(rowOf('Codex')));
    const menu = host.querySelector('[role="menu"]')!;
    expect(menu.parentElement).toBe(host);
    expect(host.querySelector('.ck-side')!.contains(menu)).toBe(false);
    expect(labels()).not.toContain('Copy selection');
  });

  describe('placement (jsdom has no layout, so the menu size is faked)', () => {
    const W = 160;
    const H = 130;
    const size = () =>
      vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
        const menu = this.getAttribute?.('role') === 'menu';
        return { x: 0, y: 0, left: 0, top: 0, right: menu ? W : 0, bottom: menu ? H : 0, width: menu ? W : 0, height: menu ? H : 0, toJSON() {} } as DOMRect;
      });
    const place = () => {
      const st = (host.querySelector('[role="menu"]') as HTMLElement).style;
      return { left: parseFloat(st.left), top: parseFloat(st.top) };
    };
    beforeEach(() => {
      Object.assign(window, { innerWidth: 1000, innerHeight: 600 });
    });

    it('opens at the pointer when it fits', async () => {
      size();
      await render(fakeApi());
      await act(async () => rightClick(rowOf('Codex'), 40, 50));
      expect(place()).toEqual({ left: 40, top: 50 });
    });

    it('is clamped inside the window on the right edge', async () => {
      size();
      await render(fakeApi());
      await act(async () => rightClick(rowOf('Codex'), 990, 50));
      expect(place()).toEqual({ left: 1000 - W - 4, top: 50 });
    });

    it('flips upward when there is no room below the pointer', async () => {
      size();
      await render(fakeApi());
      await act(async () => rightClick(rowOf('Codex'), 40, 580));
      expect(place()).toEqual({ left: 40, top: 580 - H });
    });

    it('stays inside the window when it fits neither above nor below (tiny window)', async () => {
      size();
      Object.assign(window, { innerHeight: 100 });
      await render(fakeApi());
      await act(async () => rightClick(rowOf('Codex'), 40, 60));
      expect(place().top).toBe(4);
    });
  });

  it('a click outside closes it', async () => {
    await render(fakeApi());
    await act(async () => rightClick(rowOf('Codex')));
    expect(host.querySelector('[role="menu"]')).not.toBeNull();
    await act(async () => document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })));
    expect(host.querySelector('[role="menu"]')).toBeNull();
  });
});
