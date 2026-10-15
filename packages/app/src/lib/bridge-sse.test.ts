import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeEvent } from './bridge-types';
import {
  subscribeBridgeEventsHandle,
  subscribeBridgeEventsViaPoll,
  subscribeBridgeEventsViaWs,
  type BridgeLinkStats,
} from './bridge-sse';

vi.mock('./mobile-pairing-auth', async (importOriginal) => {
  const mod = await importOriginal<typeof import('./mobile-pairing-auth')>();
  return { ...mod, mobilePairingAuthHeaders: () => ({ Authorization: 'Bearer tok' }) };
});

const terminalBlock = (paneId: string, text: string) =>
  `event: terminal\ndata: ${JSON.stringify({ pane_id: paneId, data: [...new TextEncoder().encode(text)] })}\n\n`;
const terminalB64Block = (paneId: string, text: string) =>
  `event: terminal\ndata: ${JSON.stringify({ pane_id: paneId, b64: btoa(String.fromCharCode(...new TextEncoder().encode(text))) })}\n\n`;

class MockSocket {
  static instances: MockSocket[] = [];
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public url: string, public protocols: string[]) {
    MockSocket.instances.push(this);
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  receive(data: string) {
    this.onmessage?.({ data });
  }
  fail() {
    this.readyState = 3;
    this.onclose?.();
  }
}

const BASE = 'https://x.trycloudflare.com/bridge';

describe('subscribeBridgeEventsViaPoll', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('starts from now, then delivers the payloads after each cursor and reports gaps', async () => {
    const answers = [
      { cursor: 10, data: '', gap: false },
      { cursor: 12, data: terminalBlock('p1', 'hi') + 'event: pane-status\ndata: {"pane_id":"p1","status":"running"}\n\n', gap: false },
      { cursor: 40, data: terminalBlock('p1', 'later'), gap: true },
    ];
    const urls: string[] = [];
    let drained: () => void = () => {};
    const done = new Promise<void>((resolve) => { drained = resolve; });
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      urls.push(url);
      const next = answers.shift();
      if (!next) {
        drained();
        return new Promise(() => {});
      }
      return { ok: true, json: async () => next };
    }));

    const events: BridgeEvent[] = [];
    const stop = subscribeBridgeEventsViaPoll(BASE, (e) => events.push(e));
    await done;
    stop();

    expect(urls[0]).toContain('/events/poll?wait_ms=');
    expect(urls[1]).toContain('cursor=10');
    expect(urls[2]).toContain('cursor=12');
    expect(events.map((e) => e.type)).toEqual(['terminal', 'pane-status', 'resync', 'terminal']);
  });
});

describe('subscribeBridgeEventsViaWs', () => {
  beforeEach(() => {
    MockSocket.instances = [];
    vi.stubGlobal('WebSocket', MockSocket);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('connects with the pairing token as a subprotocol and decodes b64 and legacy terminal data', () => {
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsViaWs(BASE, (e) => events.push(e));
    const sock = MockSocket.instances[0];
    expect(sock.url).toBe('wss://x.trycloudflare.com/bridge/events/ws');
    // 'tok' hex-encoded: subprotocols may not hold base64's + / = (the browser throws).
    expect(sock.protocols).toEqual(['pm-events', 'pm-bearer-hex.746f6b']);
    for (const p of sock.protocols as string[]) expect(p).toMatch(/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/);

    sock.open();
    expect(handle.isOpen()).toBe(true);
    sock.receive(terminalB64Block('p1', 'hé'));
    sock.receive(terminalBlock('p2', 'ok'));

    const [first, second] = events as Array<{ type: string; pane_id: string; data: Uint8Array | number[] }>;
    expect(first.type).toBe('terminal');
    expect(first.pane_id).toBe('p1');
    expect(first.data).toBeInstanceOf(Uint8Array);
    expect(new TextDecoder().decode(first.data as Uint8Array)).toBe('hé');
    expect(Array.from(second.data as number[])).toEqual([...new TextEncoder().encode('ok')]);
    handle.unsubscribe();
  });

  it('sends watch on open and whenever the watched set changes, and input frames when open', () => {
    const handle = subscribeBridgeEventsViaWs(BASE, () => {});
    const sock = MockSocket.instances[0];
    handle.setWatchedPanes(['p1']);
    expect(sock.sent).toEqual([]);

    sock.open();
    expect(sock.sent).toEqual([JSON.stringify({ type: 'watch', pane_ids: ['p1'] })]);

    handle.setWatchedPanes(['p1']);
    expect(sock.sent).toHaveLength(1);
    handle.setWatchedPanes(['p2']);
    expect(sock.sent[1]).toBe(JSON.stringify({ type: 'watch', pane_ids: ['p2'] }));

    expect(handle.sendInput('p2', 'ls', true)).toBe(true);
    expect(JSON.parse(sock.sent[2])).toEqual({ type: 'input', pane_id: 'p2', text: 'ls', append_newline: true });
    handle.unsubscribe();
  });

  // VERIFY-R11: a loopback PWA tab typing into an agent-run pane got `{"type":"input-error"}` and
  // only the console heard of it. It is now an event the app turns into a plain line on screen.
  it('turns an input-error frame into an input-refused event instead of dropping it', () => {
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsViaWs(BASE, (e) => events.push(e));
    const sock = MockSocket.instances[0];
    sock.open();
    sock.receive(
      JSON.stringify({ type: 'input-error', pane_id: 'p1', error: 'managed pane mutations require an MCP session id' }),
    );
    sock.receive(JSON.stringify({ type: 'input-error', pane_id: 'p1', error: 'not_allowed' }));
    expect(events).toEqual([
      { type: 'input-refused', pane_id: 'p1', error: 'managed pane mutations require an MCP session id' },
      { type: 'input-refused', pane_id: 'p1', error: 'not_allowed' },
    ]);
    handle.unsubscribe();
  });

  it('returns false from sendInput while the socket is not open', () => {
    const handle = subscribeBridgeEventsViaWs(BASE, () => {});
    expect(handle.sendInput('p1', 'x', false)).toBe(false);
    handle.unsubscribe();
  });

  it('emits resync on reconnect but not on the first open', () => {
    vi.useFakeTimers();
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsViaWs(BASE, (e) => events.push(e));
    MockSocket.instances[0].open();
    expect(events.map((e) => e.type)).toEqual([]);

    MockSocket.instances[0].fail();
    vi.advanceTimersByTime(500);
    expect(MockSocket.instances).toHaveLength(2);
    MockSocket.instances[1].open();
    expect(events.map((e) => e.type)).toEqual(['resync']);
    handle.unsubscribe();
  });

  it('falls back to long-polling after two failed opens in a row', async () => {
    vi.useFakeTimers();
    const urls: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      urls.push(url);
      return new Promise(() => {});
    }));
    const handle = subscribeBridgeEventsViaWs(BASE, () => {});
    MockSocket.instances[0].fail();
    vi.advanceTimersByTime(500);
    MockSocket.instances[1].fail();

    expect(urls.some((u) => u.includes('/events/poll?'))).toBe(true);
    expect(handle.sendInput('p1', 'x', false)).toBe(false);
    expect(MockSocket.instances).toHaveLength(2);
    handle.unsubscribe();
  });

  it('keeps retrying the socket from polling and switches back once it opens, with one resync', () => {
    vi.useFakeTimers();
    vi.stubGlobal('fetch', vi.fn(async () => new Promise(() => {})));
    const events: BridgeEvent[] = [];
    const handle = subscribeBridgeEventsViaWs(BASE, (e) => events.push(e));
    handle.setWatchedPanes(['p1']);
    MockSocket.instances[0].fail();
    vi.advanceTimersByTime(500);
    MockSocket.instances[1].fail();
    expect(handle.stats().mode).toBe('polling');

    vi.advanceTimersByTime(15_000);
    expect(MockSocket.instances).toHaveLength(3);
    MockSocket.instances[2].fail();
    expect(handle.stats().mode).toBe('polling');
    expect(events).toEqual([]);

    vi.advanceTimersByTime(15_000);
    expect(MockSocket.instances).toHaveLength(4);
    MockSocket.instances[3].open();
    expect(handle.stats().mode).toBe('socket');
    expect(handle.stats().reconnects).toBe(1);
    expect(handle.isOpen()).toBe(true);
    expect(events.map((e) => e.type)).toEqual(['resync']);
    expect(MockSocket.instances[3].sent).toEqual([JSON.stringify({ type: 'watch', pane_ids: ['p1'] })]);

    // Socket is the transport again: no further socket attempts are scheduled.
    vi.advanceTimersByTime(60_000);
    expect(MockSocket.instances).toHaveLength(4);
    handle.unsubscribe();
  });

  it('measures ping/pong round trips, keeps a median of the last ten, and notifies stats listeners', () => {
    vi.useFakeTimers();
    const now = vi.spyOn(performance, 'now').mockReturnValue(1000);
    const handle = subscribeBridgeEventsViaWs(BASE, () => {});
    const sock = MockSocket.instances[0];
    const seen: BridgeLinkStats[] = [];
    const off = handle.onStats((s) => seen.push(s));

    sock.open();
    expect(handle.stats()).toMatchObject({ mode: 'socket', rttMs: null, rttMedianMs: null, reconnects: 0 });
    expect(seen.length).toBeGreaterThan(0);

    vi.advanceTimersByTime(5000);
    expect(JSON.parse(sock.sent[0])).toEqual({ type: 'ping', t: 1000 });
    now.mockReturnValue(1085);
    sock.receive(JSON.stringify({ type: 'pong', t: 1000 }));
    expect(handle.stats().rttMs).toBe(85);
    expect(handle.stats().rttMedianMs).toBe(85);

    now.mockReturnValue(6000);
    vi.advanceTimersByTime(5000);
    now.mockReturnValue(6100);
    sock.receive(JSON.stringify({ type: 'pong', t: 6000 }));
    expect(handle.stats().rttMs).toBe(100);
    expect(handle.stats().rttMedianMs).toBe(92.5);

    off();
    const count = seen.length;
    now.mockReturnValue(9000);
    vi.advanceTimersByTime(5000);
    now.mockReturnValue(9050);
    sock.receive(JSON.stringify({ type: 'pong', t: 9000 }));
    expect(seen.length).toBe(count);
    now.mockRestore();
    handle.unsubscribe();
  });
});

describe('subscribeBridgeEventsHandle (loopback)', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('reports stream mode with no round trip for the EventSource path', () => {
    class MockEventSource {
      addEventListener() {}
      close() {}
      onerror: (() => void) | null = null;
      onopen: (() => void) | null = null;
    }
    vi.stubGlobal('EventSource', MockEventSource);
    const handle = subscribeBridgeEventsHandle('http://127.0.0.1:1234', () => {});
    expect(handle.stats()).toEqual({ mode: 'stream', rttMs: null, rttMedianMs: null, reconnects: 0 });
    handle.unsubscribe();
  });
});
