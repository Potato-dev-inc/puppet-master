import { describe, expect, it } from 'vitest';
import { Terminal } from '@xterm/xterm';
import { PaneStreamManager, type PaneScreenReplay } from './pane-stream';

/**
 * Snapshot -> live hand-over. A viewer that mounts takes the pane's screen at some stream offset
 * while output keeps arriving. Everything before the offset is in the snapshot, everything from it
 * on must be written exactly once: no byte lost (a part of a line cut off) and none repeated.
 */

const PANE = 'p';
const enc = new TextEncoder();
const flush = (term: Terminal) => new Promise<void>((resolve) => term.write('', resolve));
const tick = (ms = 0) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function lines(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < buffer.length; i++) out.push(buffer.getLine(i)?.translateToString(true) ?? '');
  while (out.length > 0 && out[out.length - 1] === '') out.pop();
  return out;
}

class FakePty {
  cols = 40;
  rows = 8;
  total = 0;
  readonly emu = new Terminal({ cols: 40, rows: 8, scrollback: 1000, allowProposedApi: true });
  readonly streams: PaneStreamManager[] = [];

  async output(text: string): Promise<void> {
    const bytes = enc.encode(text);
    const offset = this.total;
    this.total += bytes.length;
    this.emu.write(bytes);
    await flush(this.emu);
    for (const stream of this.streams) stream.ingest(PANE, bytes, offset);
  }

  async resize(cols: number, rows: number): Promise<void> {
    this.cols = cols;
    this.rows = rows;
    await flush(this.emu);
    this.emu.resize(cols, rows);
    for (const stream of this.streams) stream.ingestResize(PANE, cols, rows, this.total);
  }

  snapshot(): PaneScreenReplay {
    const buffer = this.emu.buffer.active;
    let out = '';
    for (let i = 0; i < buffer.baseY; i++) out += `${buffer.getLine(i)?.translateToString(true) ?? ''}\r\n`;
    if (buffer.baseY > 0) out += '\r\n'.repeat(this.rows - 1);
    out += '\x1b[H\x1b[2J';
    for (let row = 0; row < this.rows; row++) {
      out += `\x1b[${row + 1};1H${buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? ''}`;
    }
    out += `\x1b[${buffer.cursorY + 1};${buffer.cursorX + 1}H`;
    return { data: enc.encode(out), cols: this.cols, rows: this.rows, offset: this.total };
  }
}

function attach(pty: FakePty, reader: () => Promise<PaneScreenReplay>) {
  const stream = new PaneStreamManager();
  pty.streams.push(stream);
  const term = new Terminal({ cols: pty.cols, rows: pty.rows, scrollback: 1000, allowProposedApi: true });
  stream.subscribe(
    PANE,
    (data) => term.write(data),
    reader,
    (cols, rows) => term.write(new Uint8Array(0), () => term.resize(cols, rows)),
  );
  return { stream, term };
}

/** Output while the snapshot is in flight: chunks split mid-line, and one that straddles nothing. */
async function run(withOffset: boolean, withResize: boolean) {
  const pty = new FakePty();
  const follower = attach(pty, async () => ({ data: new Uint8Array(0), cols: 40, rows: 8, offset: 0 }));
  await tick();
  for (let i = 0; i < 20; i++) await pty.output(`before ${i}\r\n`);

  const viewer = attach(pty, async () => {
    const snap = pty.snapshot(); // the PTY reader's emulator, at this stream offset
    if (!withOffset) delete snap.offset;
    await tick(10); // the snapshot crosses IPC; output keeps coming
    return snap;
  });
  // arrives while the snapshot is in flight; the last chunk is cut in the middle of a line
  await pty.output('during A\r\n');
  await pty.output('during B ');
  await pty.output('(rest of B)\r\n');
  if (withResize) await pty.resize(30, 8);
  await pty.output('after resize C\r\n');
  await tick(30);
  await pty.output('live D\r\n');
  await flush(viewer.term);
  await flush(follower.term);
  return { viewer: viewer.term, follower: follower.term };
}

describe('snapshot to live hand-over', () => {
  it('writes every byte after the snapshot offset exactly once', async () => {
    const { viewer, follower } = await run(true, false);
    expect(lines(viewer)).toEqual(lines(follower));
    const text = lines(viewer).join('\n');
    expect(text).toContain('during B (rest of B)');
    expect(text.match(/during A/g)).toHaveLength(1);
  });

  it('keeps a resize that arrives during the hand-over in order with the bytes', async () => {
    const { viewer, follower } = await run(true, true);
    expect(viewer.cols).toBe(30);
    expect(lines(viewer)).toEqual(lines(follower));
    expect(lines(viewer).join('\n')).toContain('after resize C');
  });

  it('a snapshot without an offset loses the bytes that arrived while it loaded (the bug)', async () => {
    const { viewer, follower } = await run(false, false);
    expect(lines(viewer)).not.toEqual(lines(follower));
    expect(lines(viewer).join('\n')).not.toContain('during A');
  });
});
