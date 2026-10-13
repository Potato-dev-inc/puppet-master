import { describe, expect, it } from 'vitest';
import { Terminal } from '@xterm/xterm';
import { OwnerGrid, type GridSize } from './owner-follow';

/**
 * PC owner window -> phone lease -> reclaim. The owner window keeps `OwnerGrid` (what it asked for,
 * what the PTY has, whether it follows a phone), an xterm, and a container whose grid the fit
 * addon measures. After the reclaim the PTY, the xterm and the fitted container grid must agree,
 * and the PTY must be resized at most once.
 */

const PC = { cols: 100, rows: 30 };
const PHONE = { cols: 40, rows: 12 };
const same = (a: GridSize, b: GridSize) => a.cols === b.cols && a.rows === b.rows;
const flush = (term: Terminal) => new Promise<void>((resolve) => term.write('', resolve));

/** The app side: the PTY, the size remembered for the desktop while a phone holds it. */
class FakeApp {
  pty: GridSize;
  desktop: GridSize;
  held = false;
  ptyResizes = 0;
  constructor(initial: GridSize, private readonly echo: (g: GridSize) => void) {
    this.pty = initial;
    this.desktop = initial;
  }
  /** resize_pane from the owner: remembered while held, no echo to the owner. */
  resizePane(g: GridSize): void {
    if (this.held) {
      this.desktop = g;
      return;
    }
    if (!same(this.pty, g)) {
      this.pty = g;
      this.ptyResizes++;
    }
  }
  phoneClaims(g: GridSize): void {
    this.held = true;
    this.pty = g;
    this.ptyResizes++;
    this.echo(g);
  }
  /** reclaim_pane_size: the remembered desktop size comes back, with a pty://resized. */
  reclaim(): void {
    this.held = false;
    if (!same(this.pty, this.desktop)) {
      this.pty = this.desktop;
      this.ptyResizes++;
      this.echo(this.desktop);
    }
  }
}

/** TerminalSession's owner path, driven by hand (fitAndNotify, applyOwnerFollow, afterReclaim). */
class Owner {
  readonly grid = new OwnerGrid();
  readonly term = new Terminal({ ...PC, scrollback: 1000, allowProposedApi: true });
  readonly sent: GridSize[] = [];
  container: GridSize = PC;
  readonly app: FakeApp;

  constructor(appInitial: GridSize) {
    this.app = new FakeApp(appInitial, (g) => this.onStreamResize(g));
  }

  fit(): void {
    if (this.grid.following) {
      const send = this.grid.fitted(this.container.cols, this.container.rows);
      if (send) this.send(send);
      return;
    }
    this.term.resize(this.container.cols, this.container.rows);
    const send = this.grid.fitted(this.term.cols, this.term.rows);
    if (send) this.send(send);
  }

  private send(g: GridSize): void {
    this.sent.push(g);
    this.app.resizePane(g);
  }

  onStreamResize(g: GridSize): void {
    const d = this.grid.onStreamResize(g, { cols: this.term.cols, rows: this.term.rows });
    if (d.resizeTo) this.term.resize(d.resizeTo.cols, d.resizeTo.rows);
    if (d.refit) this.fit();
  }

  /** TerminalViewport: reclaim_pane_size resolved -> session.afterReclaim(). */
  afterReclaim(): void {
    this.grid.reclaimed();
    this.fit();
  }
}

async function scenario(setup: (o: Owner) => void) {
  const o = new Owner(PC);
  o.fit(); // mount: fit, send
  for (let i = 0; i < 80; i++) o.term.write(`line ${i}\r\n`);
  await flush(o.term);
  setup(o);
  await flush(o.term);
  return o;
}

function expectAgreement(o: Owner) {
  expect({ cols: o.term.cols, rows: o.term.rows }).toEqual(o.container);
  expect(o.app.pty).toEqual(o.container);
  expect(o.term.buffer.active.viewportY).toBe(o.term.buffer.active.baseY);
  expect(o.grid.following).toBe(false);
}

describe('owner window: phone lease then reclaim', () => {
  it('PTY, xterm and fitted grid agree after the reclaim, with one resize_pane', async () => {
    const o = await scenario((o) => {
      o.app.phoneClaims(PHONE);
      expect(o.term.cols).toBe(PHONE.cols);
      o.app.reclaim();
      o.afterReclaim();
    });
    expectAgreement(o);
    expect(o.sent).toEqual([PC]);
  });

  it('the app gives back a size another window remembered: the window re-asserts its own, once', async () => {
    const o = await scenario((o) => {
      o.app.phoneClaims(PHONE);
      o.app.desktop = { cols: 80, rows: 20 }; // remembered from the window that was open before
      o.app.reclaim();
      expect(o.term.cols).toBe(80); // followed the stale grid: the top of the screen is cut off
      o.afterReclaim();
    });
    expectAgreement(o);
    expect(o.sent).toEqual([PC, PC]);
    expect(o.app.ptyResizes).toBe(3); // phone claim, reclaim, and the one correction
  });

  it('the window shrank while the phone held the pane: the PTY comes back to the window grid', async () => {
    const o = await scenario((o) => {
      o.app.phoneClaims(PHONE);
      o.container = { cols: 70, rows: 18 };
      o.fit(); // the ResizeObserver while following: remembered by the app, grid stays at the phone's
      o.app.reclaim();
      o.afterReclaim();
    });
    expectAgreement(o);
  });

  it('without the reclaim step the stale grid sticks (the bug)', async () => {
    const o = await scenario((o) => {
      o.app.phoneClaims(PHONE);
      o.app.desktop = { cols: 80, rows: 20 };
      o.app.reclaim();
      o.fit(); // what a plain refit does while still following: deduped against lastSent
    });
    expect(o.grid.following).toBe(true);
    expect(o.term.cols).toBe(80);
    expect(o.app.pty).not.toEqual(o.container);
  });
});
