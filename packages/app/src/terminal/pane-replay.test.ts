import { describe, expect, it } from 'vitest';
import { Terminal } from '@xterm/xterm';
import { createOwnerReplayReader } from './pane-replay';
import { PaneStreamManager, type ReadRawBufferFn } from './pane-stream';

/**
 * Record + replay of a ConPTY pane that changes size (pop-out, phone lease, cockpit) while Claude
 * reprints its conversation on every resize. Invariant (proven by replaying real `claude` under
 * ConPTY into xterm.js): a viewer renders correctly only while its grid equals the PTY grid at
 * every point of the byte stream. The raw byte log has no resize boundaries, so replaying it into
 * a viewer at the current size lays old-size cursor positions over new-size text.
 */

const PANE = 'pane-1';
const enc = new TextEncoder();
const HEADER = 'CLAUDE CODE v9 - session header';
const RULE = '─';

const flush = (term: Terminal) => new Promise<void>((resolve) => term.write('', resolve));
const tick = (ms = 0) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function lines(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < buffer.length; i++) out.push(buffer.getLine(i)?.translateToString(true) ?? '');
  while (out.length > 0 && out[out.length - 1] === '') out.pop();
  return out;
}

function wrap(text: string, width: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += width) out.push(text.slice(i, i + width));
  return out;
}

/** A pane whose PTY keeps its raw output and a screen emulator at the PTY's grid. */
class FakePty {
  cols: number;
  rows: number;
  readonly raw: number[] = [];
  readonly emu: Terminal;
  private readonly streams: PaneStreamManager[] = [];

  constructor(cols: number, rows: number) {
    this.cols = cols;
    this.rows = rows;
    this.emu = new Terminal({ cols, rows, scrollback: 10_000, allowProposedApi: true });
  }

  attach(stream: PaneStreamManager): void {
    this.streams.push(stream);
  }

  async output(text: string): Promise<void> {
    const bytes = enc.encode(text);
    for (const b of bytes) this.raw.push(b);
    this.emu.write(bytes);
    await flush(this.emu);
    for (const stream of this.streams) stream.ingest(PANE, bytes);
  }

  /** The PTY changes size; `echoTo` windows are told in stream order (lease / reclaim: pty://resized). */
  async resize(cols: number, rows: number, echoTo: PaneStreamManager[]): Promise<void> {
    this.cols = cols;
    this.rows = rows;
    await flush(this.emu);
    this.emu.resize(cols, rows);
    for (const stream of echoTo) stream.ingestResize(PANE, cols, rows);
    // ConPTY repaints the visible screen from home with absolute positions after every resize.
    const base = this.emu.buffer.active.baseY;
    let repaint = '\x1b[H';
    for (let row = 0; row < rows; row++) {
      const text = this.emu.buffer.active.getLine(base + row)?.translateToString(true) ?? '';
      repaint += `\x1b[${row + 1};1H${text}\x1b[K`;
    }
    await this.output(repaint);
  }

  /** Claude Code reprints the whole conversation at the new width (no ESC[3J). */
  async claudeReprint(): Promise<void> {
    let out = `${HEADER}\r\n${RULE.repeat(this.cols)}\r\n`;
    for (let i = 0; i < 6; i++) {
      for (const line of wrap(`message ${i}: ${'lorem ipsum dolor sit amet '.repeat(6)}`, this.cols)) {
        out += `${line}\r\n`;
      }
    }
    out += `${RULE.repeat(this.cols)}\r\n`;
    await this.output(out);
  }

  /** What the backend's screen replay is: history at the current width, then the screen over it. */
  screenReplay(): { data: number[]; cols: number; rows: number } {
    const buffer = this.emu.buffer.active;
    let out = '';
    for (let i = 0; i < buffer.baseY; i++) out += `${buffer.getLine(i)?.translateToString(true) ?? ''}\r\n`;
    if (buffer.baseY > 0) out += '\r\n'.repeat(this.rows - 1);
    out += '\x1b[H\x1b[2J';
    for (let row = 0; row < this.rows; row++) {
      const text = buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? '';
      out += `\x1b[${row + 1};1H${text}`;
    }
    out += `\x1b[${buffer.cursorY + 1};${buffer.cursorX + 1}H`;
    return { data: Array.from(enc.encode(out)), cols: this.cols, rows: this.rows };
  }
}

/** What an owner window (cockpit, pop-out) does with a pane stream: TerminalSession's owner path. */
class Viewer {
  readonly term: Terminal;
  readonly stream = new PaneStreamManager();
  private unsub: (() => void) | null = null;

  constructor(pty: FakePty, cols: number, rows: number, private readonly reader: ReadRawBufferFn) {
    this.term = new Terminal({ cols, rows, scrollback: 10_000, allowProposedApi: true });
    pty.attach(this.stream);
  }

  mount(): void {
    this.unsub = this.stream.subscribe(
      PANE,
      (data) => this.term.write(data),
      this.reader,
      // applyStreamResize: after every byte before it has been parsed
      (cols, rows) => this.term.write(new Uint8Array(0), () => this.term.resize(cols, rows)),
    );
  }

  unmount(): void {
    this.unsub?.();
    this.unsub = null;
  }
}

/**
 * pop-out era at 120x30 -> phone lease 46x38 -> a window mounts at 232x64 and the PC takes the
 * size back -> Claude reprints -> switch away and back (second mount). Returns the final PTY, the
 * reference viewer (always in sync) and the remounted one.
 */
async function recordSession(makeReader: (pty: FakePty) => ReadRawBufferFn) {
  const pty = new FakePty(120, 30);
  const reference = new Viewer(pty, 120, 30, async () => []);
  reference.mount();
  await tick();
  await pty.claudeReprint();

  // phone lease: PTY 46x38, echoed in stream order to every window
  await pty.resize(46, 38, [reference.stream]);
  await pty.claudeReprint();

  // mode switch: the cockpit mounts (fit 232x64) while the phone size is still on the PTY
  const cockpit = new Viewer(pty, 232, 64, makeReader(pty));
  cockpit.mount();
  await tick(15);
  // the PC takes the size back: its own resize is not echoed to it, the reference follows
  await pty.resize(232, 64, [reference.stream]);
  await pty.claudeReprint();
  await flush(cockpit.term);

  // switch away and back: a second mount replays the pane again
  cockpit.unmount();
  await pty.claudeReprint();
  const again = new Viewer(pty, 232, 64, makeReader(pty));
  again.mount();
  await tick(15);
  await flush(again.term);
  await flush(reference.term);
  return { pty, reference, remounted: again };
}

describe('pane replay on mount', () => {
  it('raw history replayed into the current grid is garbled (the bug)', async () => {
    const { reference, remounted } = await recordSession((p) => async () => p.raw.slice());
    expect(lines(reference.term).filter((l) => l.includes(HEADER)).length).toBe(4);
    expect(lines(remounted.term)).not.toEqual(lines(reference.term));
  });

  it('screen replay equals a viewer that followed the whole stream', async () => {
    const { pty, reference, remounted } = await recordSession((p) =>
      createOwnerReplayReader({
        readScreen: async () => p.screenReplay(),
        readRaw: async () => p.raw.slice(),
        pendingResize: () => undefined,
      }),
    );
    expect(remounted.term.cols).toBe(pty.cols);
    expect(remounted.term.rows).toBe(pty.rows);
    const got = lines(remounted.term);
    const want = lines(reference.term);
    // one header per reprint, none duplicated by the replay
    expect(want.filter((l) => l.includes(HEADER)).length).toBe(4);
    expect(got.filter((l) => l.includes(HEADER)).length).toBe(4);
    expect(got).toEqual(want);
    // no narrow-wrapped copy overlaid on wide text: every rule is one width
    for (const line of got.filter((l) => l.startsWith(RULE))) expect(new Set(line).size).toBe(1);
  });

  it('draws the screen for the grid the window asked for: waits for its in-flight resize', async () => {
    const pty = new FakePty(46, 38);
    await pty.claudeReprint();
    let pending: Promise<unknown> | undefined;
    const reader = createOwnerReplayReader({
      readScreen: async () => pty.screenReplay(),
      readRaw: async () => [],
      pendingResize: () => pending,
    });
    // the window's mount-time fit asks for 232x64; the PTY applies it a moment later
    pending = tick(10).then(() => pty.resize(232, 64, []));
    const replay = await reader(PANE, 10_000);
    expect(replay).toMatchObject({ cols: 232, rows: 64 });
  });

  it('falls back to raw history when the backend has no screen replay', async () => {
    const reader = createOwnerReplayReader({
      readScreen: async () => {
        throw new Error('unknown command');
      },
      readRaw: async () => [1, 2, 3],
      pendingResize: () => undefined,
    });
    expect(await reader(PANE, 10)).toEqual([1, 2, 3]);
  });
});
