import { describe, expect, it } from 'vitest';
import { Terminal } from '@xterm/xterm';
import { CLEAR_SCROLLBACK, ReprintWipe } from './reprint-wipe';
import { PaneStreamManager } from './pane-stream';

/**
 * Claude Code reprints the whole conversation on every resize and never sends ESC[3J, so every PTY
 * resize (every phone <-> PC focus handoff) leaves one more copy in a viewer's scrollback. For
 * agents whose profile says so (`reprints_on_resize`) the viewer wipes its saved lines at the
 * resize point of the byte stream, so the reprint is the only copy. Shell-like agents keep history.
 */

const PANE = 'pane-1';
const enc = new TextEncoder();
const HEADER = 'CLAUDE CODE v9 - session header';
const RULE = '─';
const PC = { cols: 232, rows: 64 };
const PHONE = { cols: 46, rows: 38 };

const flush = (term: Terminal) => new Promise<void>((resolve) => term.write('', resolve));

function allLines(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < buffer.length; i++) out.push(buffer.getLine(i)?.translateToString(true) ?? '');
  while (out.length > 0 && out[out.length - 1] === '') out.pop();
  return out;
}

function screenLines(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const out: string[] = [];
  for (let row = 0; row < term.rows; row++) out.push(buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? '');
  return out;
}

const headers = (term: Terminal) => allLines(term).filter((l) => l.includes(HEADER)).length;

function wrap(text: string, width: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += width) out.push(text.slice(i, i + width));
  return out;
}

class FakePty {
  cols = PC.cols;
  rows = PC.rows;
  readonly emu = new Terminal({ ...PC, scrollback: 10_000, allowProposedApi: true });
  private readonly streams: PaneStreamManager[] = [];
  private n = 0;

  attach(stream: PaneStreamManager): void {
    this.streams.push(stream);
  }

  async output(text: string): Promise<void> {
    const bytes = enc.encode(text);
    this.emu.write(bytes);
    await flush(this.emu);
    for (const stream of this.streams) stream.ingest(PANE, bytes);
  }

  /** `echoTo`: windows told in stream order (pty://resized / pane-resize). The resizing owner is not. */
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
      repaint += `\x1b[${row + 1};1H${this.emu.buffer.active.getLine(base + row)?.translateToString(true) ?? ''}\x1b[K`;
    }
    await this.output(repaint);
  }

  /** Claude Code: erase its live region, then print the whole conversation at the new width. */
  async claudeReprint(): Promise<void> {
    let out = `\x1b[H\x1b[J${HEADER}\r\n${RULE.repeat(this.cols)}\r\n`;
    for (let i = 0; i < 90; i++) {
      for (const line of wrap(`message ${i}: ${'lorem ipsum dolor sit amet '.repeat(6)}`, this.cols)) {
        out += `${line}\r\n`;
      }
    }
    out += `${RULE.repeat(this.cols)}\r\n> `;
    await this.output(out);
  }

  /** A plain shell: numbered output lines, no reprint on resize. */
  async shellOutput(count: number): Promise<void> {
    let out = '';
    for (let i = 0; i < count; i++) out += `line ${String(++this.n).padStart(3, '0')}\r\n`;
    await this.output(out);
  }
}

/** A window following the pane the way TerminalSession does (stream-order resize + wipe). */
class Viewer {
  readonly term: Terminal;
  readonly stream = new PaneStreamManager();
  readonly wipe = new ReprintWipe();

  constructor(private readonly pty: FakePty, flag: boolean, size = PC) {
    this.wipe.enabled = flag;
    this.term = new Terminal({ ...size, scrollback: 10_000, allowProposedApi: true });
    pty.attach(this.stream);
    this.stream.subscribe(
      PANE,
      (data) => this.term.write(data),
      async () => [],
      // applyStreamResize: the wipe is queued in order with the bytes, the resize follows them
      (cols, rows) => {
        this.wipe.resizeTo(this.term, cols, rows);
        this.term.write(new Uint8Array(0), () => this.term.resize(cols, rows));
      },
    );
  }

  /** The owner window's own fit: the grid changes locally now and the PTY follows, with no echo. */
  async ownerFit(cols: number, rows: number): Promise<void> {
    const before = { cols: this.term.cols, rows: this.term.rows };
    this.term.resize(cols, rows);
    this.wipe.resizeTo(this.term, cols, rows, before);
    await this.pty.resize(cols, rows, []);
  }
}

/** PC -> phone -> PC -> phone, a full reprint after each resize. */
async function handoffs(pty: FakePty, viewer: Viewer, resize: (size: { cols: number; rows: number }) => Promise<void>) {
  await pty.claudeReprint();
  for (const size of [PHONE, PC, PHONE]) {
    await resize(size);
    await pty.claudeReprint();
  }
  await flush(viewer.term);
}

describe('scrollback wipe on reprint', () => {
  it('follower path: with the flag the last reprint is the only copy, without it every handoff adds one', async () => {
    const results: Record<string, number> = {};
    for (const flag of [true, false]) {
      const pty = new FakePty();
      const viewer = new Viewer(pty, flag);
      await handoffs(pty, viewer, (size) => pty.resize(size.cols, size.rows, [viewer.stream]));
      results[String(flag)] = headers(viewer.term);
      expect(viewer.term.cols).toBe(PHONE.cols);
      // the visible screen is the PTY's screen
      expect(screenLines(viewer.term)).toEqual(screenLines(pty.emu));
      if (flag) {
        // exactly one conversation: header, rules and messages once each
        const lines = allLines(viewer.term);
        expect(lines.filter((l) => l.startsWith('message 0:')).length).toBe(1);
        expect(lines.filter((l) => l.startsWith(RULE)).length).toBe(2);
        expect(new Set(lines.filter((l) => l.startsWith(RULE)).map((l) => l.length))).toEqual(new Set([PHONE.cols]));
      }
    }
    expect(results.true).toBe(1);
    expect(results.false).toBe(4);
  });

  it('owner path: the window own fit wipes at the moment its grid changes', async () => {
    const results: Record<string, number> = {};
    for (const flag of [true, false]) {
      const pty = new FakePty();
      const viewer = new Viewer(pty, flag);
      await handoffs(pty, viewer, async (size) => {
        // phone <-> PC handoffs reach the owner window as stream resizes; its own window drag is a fit
        if (size === PC) await viewer.ownerFit(size.cols, size.rows);
        else await pty.resize(size.cols, size.rows, [viewer.stream]);
      });
      results[String(flag)] = headers(viewer.term);
      expect(screenLines(viewer.term)).toEqual(screenLines(pty.emu));
    }
    expect(results.true).toBe(1);
    expect(results.false).toBe(4);
  });

  it('a resize that does not change the grid wipes nothing', async () => {
    const pty = new FakePty();
    const viewer = new Viewer(pty, true);
    await pty.claudeReprint();
    await pty.resize(PHONE.cols, PHONE.rows, [viewer.stream]);
    await pty.claudeReprint();
    await pty.resize(PHONE.cols, PHONE.rows, [viewer.stream]);
    await pty.resize(PHONE.cols, PHONE.rows, [viewer.stream]);
    await flush(viewer.term);
    expect(headers(viewer.term)).toBe(1);
    expect(viewer.wipe.resizeTo(viewer.term, PHONE.cols, PHONE.rows)).toBe(false);
  });

  it('a shell (flag off) keeps its whole history across resizes', async () => {
    const pty = new FakePty();
    const viewer = new Viewer(pty, false);
    await pty.shellOutput(150);
    for (const size of [PHONE, PC, PHONE]) {
      await pty.resize(size.cols, size.rows, [viewer.stream]);
      await pty.shellOutput(5);
    }
    await flush(viewer.term);
    const lines = allLines(viewer.term);
    expect(lines[0]).toBe('line 001');
    expect(lines.filter((l) => l.startsWith('line ')).length).toBe(165);
  });

  it('ESC[3J keeps the visible screen consistent with the PTY, term.clear() does not', async () => {
    const make = async () => {
      const pty = new FakePty();
      const viewer = new Viewer(pty, false);
      await pty.claudeReprint();
      await flush(viewer.term);
      return { pty, viewer };
    };
    const { pty, viewer } = await make();
    expect(allLines(viewer.term).length).toBeGreaterThan(viewer.term.rows);
    viewer.term.write(CLEAR_SCROLLBACK);
    await flush(viewer.term);
    expect(viewer.term.buffer.active.baseY).toBe(0);
    expect(screenLines(viewer.term)).toEqual(screenLines(pty.emu));
    expect(viewer.term.buffer.active.cursorY).toBe(pty.emu.buffer.active.cursorY);

    const other = await make();
    other.viewer.term.clear();
    await flush(other.viewer.term);
    expect(screenLines(other.viewer.term)).not.toEqual(screenLines(other.pty.emu));
  });
});
