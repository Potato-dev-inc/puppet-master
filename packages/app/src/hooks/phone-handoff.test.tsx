// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Terminal } from '@xterm/xterm';
import type { PaneInfo } from '@puppet-master/shared';
import { makeBridgeClient, type BridgeEvent } from '../lib/bridge';
import { subscribeBridgeEventsHandle, subscribeBridgeEventsViaPoll } from '../lib/bridge-sse';
import { useBridgePaneRegistry, type BridgePaneRegistryApi } from './useBridgePaneRegistry';

/**
 * The phone's snapshot -> live hand-over, end to end over the bridge's wire shapes.
 *
 * A fake PC runs the real PTY's emulator and speaks what the Rust bridge speaks: `terminal` and
 * `pane-resize` SSE payloads (carried over a WebSocket frame or a long-poll body) and
 * `GET /panes/{id}/screen`. The phone side is the production code: `makeBridgeClient`, the event
 * stream (`subscribeBridgeEventsHandle`), and `useBridgePaneRegistry` with its `PaneStreamManager`.
 * Only the three lines that route events into the registry mirror PwaApp.tsx.
 *
 * The invariant (docs/features/phone-lossless.md): whatever the timing, the phone's xterm equals the
 * PC's, with no byte of output missing and none written twice, resizes applied in stream order.
 */

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PANE = 'p1';
const BASE = 'https://phone.example'; // a remote host: the phone uses the WebSocket / long-poll path
const enc = new TextEncoder();
const flush = (term: Terminal) => new Promise<void>((resolve) => term.write('', resolve));
const tick = (ms = 0) => new Promise<void>((resolve) => setTimeout(resolve, ms));
const gate = () => {
  let open!: () => void;
  const promise = new Promise<void>((resolve) => (open = resolve));
  return { promise, open };
};

function lines(term: Terminal): string[] {
  const buffer = term.buffer.active;
  const out: string[] = [];
  for (let i = 0; i < buffer.length; i++) out.push(buffer.getLine(i)?.translateToString(true) ?? '');
  while (out.length > 0 && out[out.length - 1] === '') out.pop();
  return out;
}

class FakeSocket {
  static current: FakeSocket | null = null;
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  constructor() {
    FakeSocket.current = this;
    queueMicrotask(() => this.onopen?.());
  }
  send(): void {}
  close(): void {
    this.readyState = 3;
  }
}

type Transport = 'socket' | 'poll';

/** The PC: a PTY emulator that numbers its output and answers the bridge routes. */
class FakePc {
  cols = 40;
  rows = 8;
  total = 0;
  readonly emu = new Terminal({ cols: 40, rows: 8, scrollback: 1000, allowProposedApi: true });
  /** Every SSE payload pushed, in order (what the event log keeps for long-polling). */
  readonly log: string[] = [];
  /** false: an older build that sends no offsets. */
  numbered = true;
  /** true: events are not forwarded (a watch filter that has not taken effect yet). */
  muted = false;
  screenRequests = 0;
  /** The first /screen request waits at these gates (arrival, then reply); later ones are immediate. */
  readonly arrived = gate();
  readonly takeSnapshot = gate();
  readonly reply = gate();

  constructor(readonly transport: Transport) {}

  private push(event: string, body: Record<string, unknown>): void {
    const payload = `event: ${event}\ndata: ${JSON.stringify(body)}\n\n`;
    if (this.muted) return;
    this.log.push(payload);
    if (this.transport === 'socket') FakeSocket.current?.onmessage?.({ data: payload });
  }

  async output(text: string): Promise<void> {
    const bytes = enc.encode(text);
    const offset = this.total;
    this.total += bytes.length;
    this.emu.write(bytes);
    await flush(this.emu);
    this.push('terminal', {
      pane_id: PANE,
      b64: Buffer.from(bytes).toString('base64'),
      ...(this.numbered ? { offset } : {}),
    });
  }

  async resize(cols: number, rows: number): Promise<void> {
    this.cols = cols;
    this.rows = rows;
    await flush(this.emu);
    this.emu.resize(cols, rows);
    this.push('pane-resize', { pane_id: PANE, cols, rows, ...(this.numbered ? { offset: this.total } : {}) });
  }

  /** `GET /panes/{id}/screen`: the emulator redrawn, plus the offset it was taken at. */
  private screenBody(): Record<string, unknown> {
    const buffer = this.emu.buffer.active;
    let out = '';
    for (let i = 0; i < buffer.baseY; i++) out += `${buffer.getLine(i)?.translateToString(true) ?? ''}\r\n`;
    if (buffer.baseY > 0) out += '\r\n'.repeat(this.rows - 1);
    out += '\x1b[H\x1b[2J';
    for (let row = 0; row < this.rows; row++) {
      out += `\x1b[${row + 1};1H${buffer.getLine(buffer.baseY + row)?.translateToString(true) ?? ''}`;
    }
    out += `\x1b[${buffer.cursorY + 1};${buffer.cursorX + 1}H`;
    return {
      b64: Buffer.from(enc.encode(out)).toString('base64'),
      cols: this.cols,
      rows: this.rows,
      ...(this.numbered ? { offset: this.total } : {}),
    };
  }

  async handle(url: string, signal?: AbortSignal | null): Promise<unknown> {
    const path = url.slice(BASE.length);
    if (path === '/panes') return [pane(this)];
    if (path.startsWith(`/panes/${PANE}/screen`)) {
      this.screenRequests++;
      if (this.screenRequests === 1) {
        this.arrived.open();
        await this.takeSnapshot.promise; // the request is queued behind other work on the PC
        const body = this.screenBody();
        await this.reply.promise; // the response is on its way back over the tunnel
        return body;
      }
      const body = this.screenBody();
      await tick(15); // a retake crosses the tunnel too
      return body;
    }
    if (path.startsWith('/events/poll')) {
      const found = /cursor=(\d+)/.exec(path)?.[1];
      // The first poll (no cursor) only learns where the log is now; it carries no history.
      if (found === undefined) return { cursor: this.log.length, data: '', gap: false };
      const cursor = Number(found);
      while (this.log.length <= cursor) {
        if (signal?.aborted) throw new DOMException('aborted', 'AbortError');
        await tick(1);
      }
      return { cursor: this.log.length, data: this.log.slice(cursor).join(''), gap: false };
    }
    throw new Error(`unexpected request ${url}`);
  }
}

function pane(pc: FakePc): PaneInfo {
  return {
    id: PANE, agent_type: 'claude', pid: 1, status: 'idle', created_at: 1, last_output_at: null,
    cwd: 'C:\\w', cols: pc.cols, rows: pc.rows,
  };
}

interface Phone {
  registry: () => BridgePaneRegistryApi;
  /** The phone's xterm, wired the way TerminalSession wires a pane subscription. */
  term: Terminal;
  gaps: string[];
  open: () => void;
  close: () => void;
  stop: () => Promise<void>;
}

async function startPhone(pc: FakePc): Promise<Phone> {
  vi.stubGlobal('WebSocket', FakeSocket);
  vi.stubGlobal('fetch', async (input: string, init?: { signal?: AbortSignal | null }) => {
    const body = await pc.handle(String(input), init?.signal);
    return { ok: true, status: 200, json: async () => body, text: async () => JSON.stringify(body) };
  });
  const bridge = makeBridgeClient(BASE);
  const gaps: string[] = [];
  let latest: BridgePaneRegistryApi | null = null;
  function Probe() {
    latest = useBridgePaneRegistry(bridge, undefined, (id) => gaps.push(id));
    return null;
  }
  const root = createRoot(document.createElement('div'));
  await act(async () => root.render(<Probe />));
  const registry = () => latest!;

  // PwaApp.tsx's event routing, verbatim apart from the state setters.
  const onEvent = (ev: BridgeEvent) => {
    if (ev.type === 'terminal') registry().ingestTerminalData(ev.pane_id, ev.data, ev.offset);
    if (ev.type === 'pane-resize') {
      registry().ingestPaneResize(ev.pane_id, ev.cols, ev.rows, ev.offset);
      act(() => registry().updatePaneDimensions(ev.pane_id, ev.cols, ev.rows, true));
    }
  };
  const stopEvents =
    pc.transport === 'poll'
      ? subscribeBridgeEventsViaPoll(BASE, onEvent)
      : subscribeBridgeEventsHandle(BASE, onEvent).unsubscribe;
  await tick(5);

  const term = new Terminal({ cols: pc.cols, rows: pc.rows, scrollback: 1000, allowProposedApi: true });
  let unlisten: (() => void) | null = null;
  const open = () => {
    unlisten = registry().subscribePaneData(
      PANE,
      (data) => term.write(data),
      (cols, rows) => term.write(new Uint8Array(0), () => term.resize(cols, rows)),
    );
  };
  return {
    registry,
    term,
    gaps,
    open,
    close: () => unlisten?.(),
    stop: async () => {
      stopEvents();
      await act(async () => root.unmount());
    },
  };
}

let phone: Phone | null = null;
beforeEach(() => {
  FakeSocket.current = null;
});
afterEach(async () => {
  await phone?.stop();
  phone = null;
  vi.unstubAllGlobals();
});

/**
 * One tab open while output keeps coming:
 *   - `pre`: arrives after the request left the phone but before the PC takes the snapshot
 *   - the snapshot is taken (offset S)
 *   - `during ...`: arrives while the snapshot is still crossing the tunnel (one chunk is cut mid-line)
 *   - a lease resize in flight (optionally also one before the snapshot)
 */
async function openTabMidStream(pc: FakePc, options: { resizeBefore?: boolean; resizeAfter?: boolean } = {}) {
  for (let i = 0; i < 20; i++) await pc.output(`before ${i}\r\n`);
  phone = await startPhone(pc);
  phone.open(); // buffering starts here; the screen request leaves
  await pc.arrived.promise;
  await pc.output('pre A\r\n');
  if (options.resizeBefore) {
    await pc.resize(32, 8);
    await pc.output('pre A2\r\n');
  }
  pc.takeSnapshot.open();
  await tick(2);
  await pc.output('during B\r\n');
  await pc.output('during C (first half ');
  await pc.output('second half)\r\n');
  if (options.resizeAfter) await pc.resize(26, 8);
  await pc.output('after resize D\r\n');
  await tick(10); // let a long-poll deliver what is in flight
  pc.reply.open();
  await tick(8);
  await pc.output('just after the reply F\r\n');
  await tick(20);
  await pc.output('live E\r\n');
  await tick(5);
  await flush(phone.term);
  await flush(pc.emu);
}

describe.each(['socket', 'poll'] as const)('phone snapshot to live hand-over over the %s feed', (transport) => {
  it('writes every byte after the snapshot exactly once', async () => {
    const pc = new FakePc(transport);
    await openTabMidStream(pc);
    const text = lines(phone!.term).join('\n');
    expect(lines(phone!.term)).toEqual(lines(pc.emu));
    expect(text).toContain('during C (first half second half)');
    expect(text.match(/pre A/g)).toHaveLength(1);
    expect(text.match(/during B/g)).toHaveLength(1);
    expect(text).toContain('live E');
    expect(pc.screenRequests).toBe(1);
  });

  it('applies a lease resize in flight in order with the output', async () => {
    const pc = new FakePc(transport);
    await openTabMidStream(pc, { resizeAfter: true });
    expect(phone!.term.cols).toBe(26);
    expect(lines(phone!.term)).toEqual(lines(pc.emu));
    expect(lines(phone!.term).join('\n')).toContain('after resize D');
    // The resize is placed after the snapshot, so the screen is not stale and is not fetched again.
    expect(pc.screenRequests).toBe(1);
  });

  it('does not apply a resize the snapshot already shows a second time', async () => {
    const pc = new FakePc(transport);
    await openTabMidStream(pc, { resizeBefore: true, resizeAfter: true });
    expect(phone!.term.cols).toBe(26);
    expect(lines(phone!.term)).toEqual(lines(pc.emu));
  });

  it('retakes the screen when output between the snapshot and going live never arrived', async () => {
    // A watch filter that takes effect late: the PC drops what it pushes right after the snapshot.
    const pc = new FakePc(transport);
    for (let i = 0; i < 20; i++) await pc.output(`before ${i}\r\n`);
    phone = await startPhone(pc);
    phone.open();
    await pc.arrived.promise;
    pc.takeSnapshot.open();
    await tick(2);
    pc.muted = true;
    await pc.output('lost X\r\n');
    pc.muted = false;
    await pc.output('kept Y\r\n');
    await tick(10);
    pc.reply.open();
    await tick(30);
    await pc.output('live Z\r\n');
    await tick(5);
    await flush(phone.term);
    expect(pc.screenRequests).toBe(2);
    expect(lines(phone.term)).toEqual(lines(pc.emu));
    expect(lines(phone.term).join('\n')).toContain('lost X');
  });
});

describe('phone talking to an older backend without offsets', () => {
  it('falls back to today: bytes that arrived while the screen loaded are dropped, later ones flow', async () => {
    const pc = new FakePc('socket');
    pc.numbered = false;
    await openTabMidStream(pc);
    const text = lines(phone!.term).join('\n');
    expect(text).toContain('live E'); // the stream is live afterwards
    expect(text).not.toContain('during B'); // the old behaviour: cut off
    expect(phone!.gaps).toEqual([]); // and nothing is mistaken for a hole
    expect(pc.screenRequests).toBe(1);
  });
});

describe('live stream', () => {
  it('reports a hole in the numbering so the viewer can redraw from the PC', async () => {
    const pc = new FakePc('socket');
    for (let i = 0; i < 5; i++) await pc.output(`before ${i}\r\n`);
    phone = await startPhone(pc);
    phone.open();
    await pc.arrived.promise;
    pc.takeSnapshot.open();
    pc.reply.open();
    await tick(20);
    await pc.output('ok 1\r\n');
    await pc.output('ok 2\r\n');
    expect(phone.gaps).toEqual([]);
    pc.muted = true;
    await pc.output('swallowed\r\n');
    pc.muted = false;
    await pc.output('after the hole\r\n');
    expect(phone.gaps).toEqual([PANE]);
  });
});
