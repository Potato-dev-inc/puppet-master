import type { McpLogEntry, OrchestratorChatEvent, PaneInfo } from '@puppet-master/shared';
import type { PublicSettings } from './bridge-settings';
import { ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders } from './mobile-pairing-auth';
import type { BridgeEvent } from './bridge';

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
