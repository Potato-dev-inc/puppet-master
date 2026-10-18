// @vitest-environment jsdom
// Every PTY resize makes an inline TUI reprint its screen, so the owner terminal must send one only
// when its grid really changed. A reflow nudge, a pane scrolling into view or a ResizeObserver tick at
// an unchanged size must send nothing; the alternate-screen redraw (rows-1 then rows) is only for an
// explicit request.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const world = vi.hoisted(() => ({
  container: { cols: 100, rows: 30 },
  buffer: 'normal' as 'normal' | 'alternate',
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
    constructor(opts: Record<string, unknown>) {
      this.options = opts;
    }
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
      return { ...world.container };
    }
    fit() {
      this.terminal?.resize(world.container.cols, world.container.rows);
    }
  },
}));
vi.mock('@xterm/addon-web-links', () => ({ WebLinksAddon: class {} }));
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }));
vi.mock('@xterm/xterm/css/xterm.css', () => ({}));

import { TerminalSession } from './terminal-session';

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

let container: HTMLElement;
let sent: Array<[number, number]>;
let session: TerminalSession;

const frames = (n = 3) => {
  for (let i = 0; i < n; i += 1) vi.advanceTimersByTime(20);
};

function mount(assumePtyGrid?: { cols: number; rows: number }) {
  session = new TerminalSession({
    paneId: 'p1',
    syncPTYResize: true,
    onResize: (cols, rows) => sent.push([cols, rows]),
    onInput: () => undefined,
    assumePtyGrid,
  });
  session.mount(container, () => () => undefined);
  frames();
}

beforeEach(() => {
  vi.useFakeTimers();
  world.container = { cols: 100, rows: 30 };
  world.buffer = 'normal';
  world.resizeObservers.length = 0;
  world.intersectionObservers.length = 0;
  (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;
  (globalThis as { IntersectionObserver?: unknown }).IntersectionObserver = FakeIntersectionObserver;
  container = document.createElement('div');
  document.body.appendChild(container);
  sent = [];
});
afterEach(() => {
  session?.dispose();
  container.remove();
  vi.useRealTimers();
});

describe('TerminalSession sends a PTY resize only when the grid really changed', () => {
  it('sends the fitted grid once on mount', () => {
    mount();
    expect(sent).toEqual([[100, 30]]);
  });

  it('a plain reflow at an unchanged size sends nothing, even on the alternate screen', () => {
    mount();
    world.buffer = 'alternate';
    sent.length = 0;
    expect(session.nudgeReflow()).toBe(true);
    frames();
    expect(sent).toEqual([]);
  });

  it('a pane scrolling into view sends nothing, even on the alternate screen', () => {
    mount();
    world.buffer = 'alternate';
    sent.length = 0;
    for (const notify of world.intersectionObservers) notify([{ isIntersecting: true }]);
    frames(10);
    expect(sent).toEqual([]);
  });

  it('a ResizeObserver tick at an unchanged size sends nothing', () => {
    mount();
    sent.length = 0;
    for (const tick of world.resizeObservers) tick();
    frames();
    expect(sent).toEqual([]);
  });

  it('a remount that already knows the PTY grid sends nothing when it fits the same grid', () => {
    mount({ cols: 100, rows: 30 });
    expect(sent).toEqual([]);
  });

  it('a real size change sends exactly one resize', () => {
    mount();
    sent.length = 0;
    world.container = { cols: 120, rows: 34 };
    for (const tick of world.resizeObservers) tick();
    frames();
    expect(sent).toEqual([[120, 34]]);
  });

  it('the alternate-screen redraw is explicit: one row down and back', () => {
    mount();
    world.buffer = 'alternate';
    sent.length = 0;
    session.nudgeReflow({ redrawAltBuffer: true });
    frames();
    expect(sent).toEqual([[100, 29], [100, 30]]);
  });

  it('an explicit redraw on the normal screen sends nothing', () => {
    mount();
    sent.length = 0;
    session.nudgeReflow({ redrawAltBuffer: true });
    frames();
    expect(sent).toEqual([]);
  });
});
