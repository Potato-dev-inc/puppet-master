// Bridge event stream: native EventSource (SSE) for loopback, long-polling `/events/poll` for remote
// hosts (Cloudflare quick tunnels do not pass SSE through at all).
import type { McpLogEntry, OrchestratorChatEvent, PaneInfo } from '@puppet-master/shared';
import type { PublicSettings } from './bridge-settings';
import { isNgrokHost, ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders, mobilePairingAuthHeaders } from './mobile-pairing-auth';
import type { BridgeEvent, OpenCodeWorkerEvent } from './bridge-types';

/** Decode a base64 terminal chunk into raw bytes. */
export function decodeBase64(text: string): Uint8Array {
  const bin = atob(text);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function dispatchBridgeEvent(eventName: string, data: string, onEvent: (e: BridgeEvent) => void): void {
  const parsed = JSON.parse(data) as unknown;
  if (eventName === 'panes') {
    onEvent({ type: 'panes', panes: parsed as PaneInfo[] });
  } else if (eventName === 'log') {
    onEvent({ type: 'log', entry: parsed as McpLogEntry });
  } else if (eventName === 'chat') {
    onEvent({ type: 'chat', event: parsed as OrchestratorChatEvent });
  } else if (eventName === 'terminal') {
    // Current servers send `b64`; older builds sent a `data` byte array.
    const payload = parsed as { pane_id: string; b64?: string; data?: number[] };
    const bytes = typeof payload.b64 === 'string' ? decodeBase64(payload.b64) : (payload.data ?? []);
    onEvent({ type: 'terminal', pane_id: payload.pane_id, data: bytes });
  } else if (eventName === 'terminal-snapshot') {
    const payload = parsed as { pane_id: string; snapshot: string };
    onEvent({ type: 'terminal-snapshot', pane_id: payload.pane_id, snapshot: payload.snapshot });
  } else if (eventName === 'pane-status') {
    const payload = parsed as { pane_id: string; status: PaneInfo['status'] };
    onEvent({ type: 'pane-status', pane_id: payload.pane_id, status: payload.status });
  } else if (eventName === 'opencode-worker') {
    onEvent({ type: 'opencode-worker', event: parsed as OpenCodeWorkerEvent });
  } else if (eventName === 'pane-resize') {
    const payload = parsed as { pane_id: string; cols: number; rows: number };
    onEvent({
      type: 'pane-resize',
      pane_id: payload.pane_id,
      cols: payload.cols,
      rows: payload.rows,
    });
  } else if (eventName === 'settings') {
    onEvent({ type: 'settings', settings: parsed as PublicSettings });
  } else if (eventName === 'orchestrator-viewport') {
    const payload = parsed as { width: number; height: number; active: boolean };
    onEvent({
      type: 'orchestrator-viewport',
      width: payload.width,
      height: payload.height,
      active: payload.active,
    });
  }
}

function parseSseChunk(
  chunk: string,
  onEvent: (e: BridgeEvent) => void,
): string {
  const parts = chunk.split('\n\n');
  const remainder = parts.pop() ?? '';

  for (const block of parts) {
    if (!block.trim() || block.startsWith(':')) continue;
    let eventName = 'message';
    let data = '';
    for (const line of block.split('\n')) {
      if (line.startsWith('event:')) eventName = line.slice(6).trim();
      else if (line.startsWith('data:')) data += line.slice(5).trim();
    }
    if (!data) continue;
    try {
      dispatchBridgeEvent(eventName, data, onEvent);
    } catch {
      /* ignore malformed blocks */
    }
  }

  return remainder;
}

/**
 * EventSource cannot set headers (required for ngrok free tier). Use fetch streaming instead.
 */
export function subscribeBridgeEventsViaFetch(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  let cancelled = false;
  let retryDelay = 500;
  let abort: AbortController | null = null;

  async function connect() {
    while (!cancelled) {
      abort = new AbortController();
      try {
        const res = await fetch(`${baseUrl}/events`, {
          headers: mergeBridgeHeaders(ngrokRequestHeaders(baseUrl)),
          signal: abort.signal,
        });
        if (!res.ok || !res.body) {
          throw new Error(`bridge SSE fetch failed: ${res.status}`);
        }

        retryDelay = 500;
        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (!cancelled) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          buffer = parseSseChunk(buffer, onEvent);
        }
      } catch (err) {
        if (cancelled) return;
        onError?.(err);
        await new Promise((r) => setTimeout(r, retryDelay));
        retryDelay = Math.min(retryDelay * 2, 5000);
      } finally {
        abort = null;
      }
    }
  }

  void connect();

  return () => {
    cancelled = true;
    abort?.abort();
  };
}

interface EventPoll {
  cursor: number;
  data: string;
  gap: boolean;
}

/** How long one `/events/poll` request waits for events before answering empty. */
const POLL_WAIT_MS = 20_000;

/**
 * Long-poll `GET /events/poll`: same payloads as the SSE stream, delivered as ordinary requests
 * that any proxy or tunnel passes through. Each request returns as soon as there are events after
 * the cursor; `gap` (events dropped while away) is surfaced as a `resync` event.
 */
export function subscribeBridgeEventsViaPoll(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  let cancelled = false;
  let retryDelay = 500;
  let abort: AbortController | null = null;
  let cursor: number | null = null;

  async function loop() {
    while (!cancelled) {
      abort = new AbortController();
      try {
        const query = cursor === null ? `wait_ms=${POLL_WAIT_MS}` : `cursor=${cursor}&wait_ms=${POLL_WAIT_MS}`;
        const res = await fetch(`${baseUrl}/events/poll?${query}`, {
          headers: mergeBridgeHeaders(ngrokRequestHeaders(baseUrl)),
          signal: abort.signal,
          cache: 'no-store',
        });
        if (!res.ok) throw new Error(`bridge event poll failed: ${res.status}`);
        const body = (await res.json()) as EventPoll;
        if (body.gap && cursor !== null) onEvent({ type: 'resync' });
        cursor = body.cursor;
        if (body.data) parseSseChunk(body.data, onEvent);
        retryDelay = 500;
      } catch (err) {
        if (cancelled) return;
        onError?.(err);
        await new Promise((r) => setTimeout(r, retryDelay));
        retryDelay = Math.min(retryDelay * 2, 5000);
      } finally {
        abort = null;
      }
    }
  }

  void loop();

  return () => {
    cancelled = true;
    abort?.abort();
  };
}

const BRIDGE_EVENT_NAMES = [
  'panes',
  'log',
  'chat',
  'terminal',
  'terminal-snapshot',
  'pane-status',
  'opencode-worker',
  'pane-resize',
  'settings',
  'orchestrator-viewport',
] as const;

function shouldUseFetchBridgeSse(baseUrl: string): boolean {
  if (isNgrokHost(baseUrl)) return true;
  try {
    const { hostname } = new URL(baseUrl);
    return hostname !== '127.0.0.1' && hostname !== 'localhost';
  } catch {
    return false;
  }
}

/** How the phone reaches the bridge right now. */
export type BridgeLinkMode = 'socket' | 'polling' | 'stream' | 'connecting';

export interface BridgeLinkStats {
  mode: BridgeLinkMode;
  /** Latest socket round trip (ping to pong), ms. */
  rttMs: number | null;
  /** Median of the last 10 round trips, ms. */
  rttMedianMs: number | null;
  /** Socket reconnects, including recoveries from polling, since subscribing. */
  reconnects: number;
}

/** Live handle to the event stream: watch filter, input over the socket, link stats, and teardown. */
export interface BridgeEventsHandle {
  unsubscribe(): void;
  /** Only `terminal` events for these panes are sent. `null` leaves the server default untouched. */
  setWatchedPanes(ids: string[] | null): void;
  /** Writes input over the socket. Returns false when the socket is not open (caller falls back to POST). */
  sendInput(paneId: string, text: string, appendNewline: boolean): boolean;
  isOpen(): boolean;
  stats(): BridgeLinkStats;
  /** Called with a fresh snapshot when the stats change, at most about once a second. Returns an unsubscribe. */
  onStats(cb: (stats: BridgeLinkStats) => void): () => void;
}

const WS_OPEN = 1;
/** Failed opens in a row, before the socket ever opened, that switch to long-polling. */
const WS_FAILS_BEFORE_POLL = 2;
/** While on long-polling, how often the socket is tried again (one attempt at a time). */
const WS_RECOVER_EVERY_MS = 15_000;
const WS_PING_EVERY_MS = 5_000;
const RTT_WINDOW = 10;
const STATS_MIN_GAP_MS = 1_000;

function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function bridgeSocketUrl(baseUrl: string): string {
  const url = new URL(`${baseUrl}/events/ws`);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.toString();
}

/** Browsers cannot set headers on a WebSocket, so the bearer token travels as a subprotocol. */
function bridgeSocketProtocols(): string[] {
  const auth = mobilePairingAuthHeaders().Authorization;
  const token = auth?.startsWith('Bearer ') ? auth.slice(7).trim() : '';
  return token ? ['pm-events', `pm-bearer.${token}`] : ['pm-events'];
}

/**
 * Remote event stream over WebSocket `/events/ws`. Reconnects with backoff; every reconnect emits
 * `resync`. After {@link WS_FAILS_BEFORE_POLL} failed opens in a row (old build, proxy without ws)
 * it runs {@link subscribeBridgeEventsViaPoll} and keeps retrying the socket every
 * {@link WS_RECOVER_EVERY_MS}. When the socket opens again the poll loop stops and a `resync` is emitted.
 * Pings every {@link WS_PING_EVERY_MS} measure the round trip.
 */
export function subscribeBridgeEventsViaWs(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): BridgeEventsHandle {
  let stopped = false;
  let fellBack = false;
  let everOpened = false;
  let failures = 0;
  let retryDelay = 500;
  let ws: WebSocket | null = null;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let recoverTimer: ReturnType<typeof setInterval> | null = null;
  let pingTimer: ReturnType<typeof setInterval> | null = null;
  let pollStop: (() => void) | null = null;
  let watched: string[] | null = null;
  let watchedKey = '\0unset';

  let mode: BridgeLinkMode = 'connecting';
  let reconnects = 0;
  let rttMs: number | null = null;
  const rtts: number[] = [];
  const listeners = new Set<(stats: BridgeLinkStats) => void>();
  let lastNotify = Number.NEGATIVE_INFINITY;
  let notifyTimer: ReturnType<typeof setTimeout> | null = null;

  function snapshot(): BridgeLinkStats {
    return { mode, rttMs, rttMedianMs: median(rtts), reconnects };
  }

  function notifyStats(): void {
    if (listeners.size === 0) return;
    const wait = lastNotify + STATS_MIN_GAP_MS - Date.now();
    if (wait > 0) {
      if (!notifyTimer) {
        notifyTimer = setTimeout(() => {
          notifyTimer = null;
          notifyStats();
        }, wait);
      }
      return;
    }
    lastNotify = Date.now();
    const next = snapshot();
    for (const listener of listeners) listener(next);
  }

  function setMode(next: BridgeLinkMode): void {
    if (mode === next) return;
    mode = next;
    notifyStats();
  }

  function sendWatch(): void {
    if (!ws || ws.readyState !== WS_OPEN || !watched) return;
    ws.send(JSON.stringify({ type: 'watch', pane_ids: watched }));
  }

  function sendPing(): void {
    if (!ws || ws.readyState !== WS_OPEN) return;
    try {
      ws.send(JSON.stringify({ type: 'ping', t: performance.now() }));
    } catch {
      /* socket closing; the close handler takes over */
    }
  }

  function stopPing(): void {
    if (pingTimer) clearInterval(pingTimer);
    pingTimer = null;
  }

  function recordRtt(ms: number): void {
    if (!(ms >= 0)) return;
    rttMs = ms;
    rtts.push(ms);
    if (rtts.length > RTT_WINDOW) rtts.shift();
    notifyStats();
  }

  function handleFrame(text: string): void {
    if (text.startsWith('{')) {
      try {
        const msg = JSON.parse(text) as { type?: string; pane_id?: string; error?: string; t?: number };
        if (msg.type === 'input-error') console.warn('[bridge] input rejected', msg.pane_id, msg.error);
        else if (msg.type === 'pong' && typeof msg.t === 'number') recordRtt(performance.now() - msg.t);
      } catch {
        /* ignore malformed control frame */
      }
      return;
    }
    parseSseChunk(text.endsWith('\n\n') ? text : `${text}\n\n`, onEvent);
  }

  function fallBackToPoll(): void {
    fellBack = true;
    setMode('polling');
    pollStop = subscribeBridgeEventsViaPoll(baseUrl, onEvent, onError);
    if (!recoverTimer) recoverTimer = setInterval(() => connect(), WS_RECOVER_EVERY_MS);
  }

  function scheduleReconnect(): void {
    if (stopped) return;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      connect();
    }, retryDelay);
    retryDelay = Math.min(retryDelay * 2, 5000);
  }

  function connect(): void {
    // One socket attempt at a time: while one is connecting or open, do nothing.
    if (stopped || ws) return;
    let sock: WebSocket;
    try {
      sock = new WebSocket(bridgeSocketUrl(baseUrl), bridgeSocketProtocols());
    } catch (err) {
      onError?.(err);
      if (!fellBack) {
        failures++;
        if (!everOpened && failures >= WS_FAILS_BEFORE_POLL) fallBackToPoll();
        else scheduleReconnect();
      }
      return;
    }
    ws = sock;
    sock.onopen = () => {
      if (sock !== ws) return;
      const wasPolling = fellBack;
      const reconnect = everOpened;
      everOpened = true;
      failures = 0;
      retryDelay = 500;
      if (wasPolling) {
        pollStop?.();
        pollStop = null;
        fellBack = false;
        if (recoverTimer) clearInterval(recoverTimer);
        recoverTimer = null;
      }
      sendWatch();
      stopPing();
      pingTimer = setInterval(sendPing, WS_PING_EVERY_MS);
      if (reconnect || wasPolling) {
        reconnects++;
        onEvent({ type: 'resync' });
      }
      setMode('socket');
      notifyStats();
    };
    sock.onmessage = (ev) => {
      if (sock !== ws || typeof ev.data !== 'string') return;
      try {
        handleFrame(ev.data);
      } catch (err) {
        onError?.(err);
      }
    };
    sock.onclose = () => {
      if (sock !== ws) return;
      ws = null;
      stopPing();
      if (stopped) return;
      onError?.(new Error('bridge events socket closed'));
      if (fellBack) {
        // The recovery timer tries again; polling keeps the events flowing meanwhile.
        setMode('polling');
        return;
      }
      setMode('connecting');
      if (!everOpened) {
        failures++;
        if (failures >= WS_FAILS_BEFORE_POLL) {
          fallBackToPoll();
          return;
        }
      }
      scheduleReconnect();
    };
  }

  connect();

  return {
    unsubscribe() {
      stopped = true;
      if (retryTimer) clearTimeout(retryTimer);
      retryTimer = null;
      if (recoverTimer) clearInterval(recoverTimer);
      recoverTimer = null;
      if (notifyTimer) clearTimeout(notifyTimer);
      notifyTimer = null;
      stopPing();
      listeners.clear();
      const sock = ws;
      ws = null;
      sock?.close();
      pollStop?.();
      pollStop = null;
    },
    setWatchedPanes(ids) {
      const key = ids === null ? '\0null' : ids.join('\n');
      if (key === watchedKey) return;
      watchedKey = key;
      watched = ids === null ? null : [...ids];
      sendWatch();
    },
    sendInput(paneId, text, appendNewline) {
      if (!ws || ws.readyState !== WS_OPEN) return false;
      try {
        ws.send(JSON.stringify({ type: 'input', pane_id: paneId, text, append_newline: appendNewline }));
        return true;
      } catch {
        return false;
      }
    },
    isOpen() {
      return !!ws && ws.readyState === WS_OPEN;
    },
    stats: snapshot,
    onStats(cb) {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
  };
}

/** Native EventSource stream (loopback only). Returns an unlisten function. */
function subscribeBridgeEventsViaSse(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  let es: EventSource | null = null;
  let cancelled = false;
  let retryDelay = 500;

  function connect() {
    if (cancelled) return;
    es = new EventSource(`${baseUrl}/events`);
    for (const name of BRIDGE_EVENT_NAMES) {
      es.addEventListener(name, (ev) => {
        try {
          dispatchBridgeEvent(name, (ev as MessageEvent).data as string, onEvent);
        } catch (err) {
          onError?.(err);
        }
      });
    }
    es.onerror = () => {
      es?.close();
      es = null;
      if (cancelled) return;
      setTimeout(connect, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 5000);
    };
    es.onopen = () => {
      retryDelay = 500;
    };
  }

  connect();

  return () => {
    cancelled = true;
    es?.close();
  };
}

/**
 * Full event-stream handle for the mobile PWA. Remote hosts use the WebSocket; loopback uses
 * EventSource, which has no watch filter and no socket input (the handle reports so).
 */
export function subscribeBridgeEventsHandle(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): BridgeEventsHandle {
  if (shouldUseFetchBridgeSse(baseUrl)) {
    return subscribeBridgeEventsViaWs(baseUrl, onEvent, onError);
  }
  const stop = subscribeBridgeEventsViaSse(baseUrl, onEvent, onError);
  return {
    unsubscribe: stop,
    setWatchedPanes: () => {},
    sendInput: () => false,
    isOpen: () => false,
    stats: () => ({ mode: 'stream', rttMs: null, rttMedianMs: null, reconnects: 0 }),
    onStats: () => () => {},
  };
}

/**
 * Subscribe to the bridge event stream. Returns an unlisten function.
 * Remote hosts use the WebSocket (with long-poll fallback); loopback uses EventSource.
 */
export function subscribeBridgeEvents(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  return subscribeBridgeEventsHandle(baseUrl, onEvent, onError).unsubscribe;
}
