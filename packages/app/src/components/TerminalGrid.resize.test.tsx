// @vitest-environment jsdom
// The whole stack (grid -> pane -> useTerminalSession -> TerminalSession) with a fake xterm and a spy on
// the bridge's resize call: a pane whose cell did not change size must never get a PTY resize, whatever
// happens around it. Every resize makes an inline TUI (Claude Code and friends) reprint its screen.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneData, PaneRegistryApi } from '../hooks/usePaneRegistry';

const world = vi.hoisted(() => ({
  grid: { cols: 100, rows: 30 },
  buffer: 'normal' as 'normal' | 'alternate',
  resizeCalls: [] as Array<[string, number, number]>,
  resizeObservers: [] as Array<() => void>,
  intersectionObservers: [] as Array<(entries: Array<{ isIntersecting: boolean }>) => void>,
}));

vi.mock('@xterm/xterm', () => {
  class Terminal {
    cols = 80;
    rows = 24;
    element = document.createElement('div');
    options: Record<string, unknown> = {};
    unicode = { activeVersion: '6' };
    private resizeListeners: Array<() => void> = [];
    get buffer() {
      return { active: { type: world.buffer, viewportY: 0, baseY: 0, cursorY: 0 } };
    }
    loadAddon(addon: { activate?: (t: Terminal) => void }) {
      addon.activate?.(this);
    }
    open(target: HTMLElement) {
      target.appendChild(this.element);
    }
    onResize(cb: () => void) {
      this.resizeListeners.push(cb);
      return { dispose() {} };
    }
    onData() {
      return { dispose() {} };
    }
    resize(cols: number, rows: number) {
      this.cols = cols;
      this.rows = rows;
      for (const cb of this.resizeListeners) cb();
    }
    write(_data: unknown, cb?: () => void) {
      cb?.();
    }
    registerLinkProvider() {
      return { dispose() {} };
    }
    refresh() {}
    clearTextureAtlas() {}
    scrollToBottom() {}
    focus() {}
    paste() {}
    reset() {}
    getSelection() {
      return '';
    }
    dispose() {}
  }
  return { Terminal };
});
vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    private terminal: { resize: (c: number, r: number) => void } | null = null;
    activate(t: { resize: (c: number, r: number) => void }) {
      this.terminal = t;
    }
    proposeDimensions() {
      return { ...world.grid };
    }
    fit() {
      this.terminal?.resize(world.grid.cols, world.grid.rows);
    }
  },
}));
vi.mock('@xterm/addon-web-links', () => ({ WebLinksAddon: class {} }));
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }));
vi.mock('@xterm/xterm/css/xterm.css', () => ({}));
vi.mock('../lib/tauri', () => ({
  tauri: {
    resize: vi.fn(async (paneId: string, cols: number, rows: number) => {
      world.resizeCalls.push([paneId, cols, rows]);
    }),
    writeInput: vi.fn(),
    reclaimPaneSize: vi.fn(async () => undefined),
  },
}));
vi.mock('../lib/agentDefinitions', () => ({
  errorMessage: (e: unknown) => String(e),
  tauriRecorderApi: { status: async () => ({ recording: false }), start: async () => undefined, stop: async () => ({ path: '' }) },
}));
vi.mock('../lib/detached-pane-window', () => ({
  detachedWindowSizeFromGrid: () => ({ width: 800, height: 600 }),
  openDetachedPaneWindow: vi.fn(async () => undefined),
}));

import { TerminalGrid } from './TerminalGrid';

class FakeResizeObserver {
  constructor(cb: () => void) {
    world.resizeObservers.push(cb);
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}
class FakeIntersectionObserver {
  constructor(cb: (entries: Array<{ isIntersecting: boolean }>) => void) {
    world.intersectionObservers.push(cb);
  }
  observe() {}
  unobserve() {}
  disconnect() {}
}

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function pane(id: string, status = 'idle'): PaneData {
  return {
    status: status as PaneData['status'],
    info: { id, agent_type: 'claude', status, pid: 1, created_at: 1, cwd: 'proj', cols: 100, rows: 30 } as PaneData['info'],
  };
}
function registry(list: PaneData[]): PaneRegistryApi {
  return {
    panes: new Map(list.map((p) => [p.info.id, p])),
    paneList: list,
    initialReady: true,
    spawnPane: vi.fn(),
    replacePaneAgent: vi.fn(),
    subscribePaneData: () => () => undefined,
  } as unknown as PaneRegistryApi;
}
// One stable subscription, like the real registry's useCallback: a new one per render would remount every pane.
const subscribe = () => () => undefined;
const withSubscribe = (r: PaneRegistryApi) => ({ ...r, subscribePaneData: subscribe }) as PaneRegistryApi;

let host: HTMLDivElement;
let root: Root;
const noop = () => undefined;
const resizesOf = (id: string) => world.resizeCalls.filter(([pane]) => pane === id);

function render(list: PaneData[], detached: string[] = []) {
  return act(async () => {
    root.render(
      <TerminalGrid registry={withSubscribe(registry(list))} projectPath="proj" onClosePane={noop} detachedPaneIds={new Set(detached)} />,
    );
  });
}
// Real time passes between steps (rAF drives xterm's deferred open); the settle animation lasts 420 ms.
const settle = (ms = 700) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => {
  vi.useFakeTimers();
  world.grid = { cols: 100, rows: 30 };
  world.buffer = 'normal';
  world.resizeCalls.length = 0;
  world.resizeObservers.length = 0;
  world.intersectionObservers.length = 0;
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;
  (globalThis as { IntersectionObserver?: unknown }).IntersectionObserver = FakeIntersectionObserver;
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('a pane whose cell did not change size never gets a PTY resize', () => {
  it('gives each pane its fitted grid once when it opens', async () => {
    await render([pane('a')]);
    await settle();
    expect(resizesOf('a')).toEqual([['a', 100, 30]]);
  });

  it('other panes opening and closing send the existing pane nothing, on the normal or the alternate screen', async () => {
    for (const buffer of ['normal', 'alternate'] as const) {
      world.resizeCalls.length = 0;
      world.buffer = buffer;
      await render([pane('a')]);
      await settle();
      world.resizeCalls.length = 0;

      await render([pane('a'), pane('b')]);
      await settle();
      await render([pane('a'), pane('b'), pane('c')]);
      await settle();
      await render([pane('a'), pane('c')]);
      await settle();
      await render([pane('a')]);
      await settle();

      expect(resizesOf('a'), `${buffer} screen`).toEqual([]);
      await act(async () => root.render(<div />));
    }
  });

  it('status updates, a scroll into view and unchanged ResizeObserver ticks send nothing either', async () => {
    world.buffer = 'alternate';
    await render([pane('a'), pane('b')]);
    await settle();
    world.resizeCalls.length = 0;

    await render([pane('a', 'running'), pane('b', 'waiting_input')]);
    await render([pane('a', 'idle'), pane('b', 'idle')]);
    for (const notify of world.intersectionObservers) notify([{ isIntersecting: true }]);
    for (const tick of world.resizeObservers) tick();
    await settle();

    expect(world.resizeCalls).toEqual([]);
  });

  it('a pane returning from its own window is the only one that is re-fitted and redrawn', async () => {
    world.buffer = 'alternate';
    await render([pane('a'), pane('b')], ['b']);
    await settle();
    world.resizeCalls.length = 0;

    await render([pane('a'), pane('b')], []);
    await settle();

    expect(resizesOf('a')).toEqual([]);
    const b = resizesOf('b');
    expect(b.length).toBeGreaterThan(0);
    expect(b[0]).toEqual(['b', 100, 30]);
    // The alternate-screen redraw (rows-1, rows) is for the returned pane only.
    expect(b.some(([, , rows]) => rows === 29)).toBe(true);
  });

  it('a cell that really changes size is re-fitted once (drag-to-resize keeps working)', async () => {
    await render([pane('a')]);
    await settle();
    world.resizeCalls.length = 0;
    world.grid = { cols: 120, rows: 34 };
    for (const tick of world.resizeObservers) tick();
    await settle();
    expect(resizesOf('a')).toEqual([['a', 120, 34]]);
  });
});
