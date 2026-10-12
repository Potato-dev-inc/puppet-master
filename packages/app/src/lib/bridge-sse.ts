// Bridge event stream: native EventSource (SSE) for loopback, long-polling `/events/poll` for remote
// hosts (Cloudflare quick tunnels do not pass SSE through at all).
import type { McpLogEntry, OrchestratorChatEvent, PaneInfo } from '@puppet-master/shared';
import type { PublicSettings } from './bridge-settings';
import { isNgrokHost, ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders } from './mobile-pairing-auth';
import type { BridgeEvent, OpenCodeWorkerEvent } from './bridge-types';

function dispatchBridgeEvent(eventName: string, data: string, onEvent: (e: BridgeEvent) => void): void {
  const parsed = JSON.parse(data) as unknown;
  if (eventName === 'panes') {
    onEvent({ type: 'panes', panes: parsed as PaneInfo[] });
  } else if (eventName === 'log') {
    onEvent({ type: 'log', entry: parsed as McpLogEntry });
  } else if (eventName === 'chat') {
    onEvent({ type: 'chat', event: parsed as OrchestratorChatEvent });
  } else if (eventName === 'terminal') {
    const payload = parsed as { pane_id: string; data: number[] };
    onEvent({ type: 'terminal', pane_id: payload.pane_id, data: payload.data });
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

/**
 * Subscribe to the bridge SSE stream. Returns an unlisten function.
 * If the bridge is unreachable, retries forever with exponential backoff.
 */
export function subscribeBridgeEvents(
  baseUrl: string,
  onEvent: (e: BridgeEvent) => void,
  onError?: (err: unknown) => void,
): () => void {
  if (shouldUseFetchBridgeSse(baseUrl)) {
    return subscribeBridgeEventsViaPoll(baseUrl, onEvent, onError);
  }

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
