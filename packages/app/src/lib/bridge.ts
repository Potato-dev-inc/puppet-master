import type { McpLogEntry, PaneInfo } from '@puppet-master/shared';

const DEFAULT_POLL_HOST = '127.0.0.1';
const DEFAULT_POLL_INTERVAL_MS = 200;

/**
 * Discover the bridge URL by reading the port file written by the GUI on start.
 * Returns null if the file doesn't exist yet.
 */
export async function discoverBridge(): Promise<string | null> {
  try {
    const res = await fetch('/__puppet_master_bridge__.json', { cache: 'no-store' });
    if (res.ok) {
      const j = (await res.json()) as { url?: string };
      if (j.url) return j.url;
    }
  } catch {
    /* ignore — we don't actually expose this in dev, fall through */
  }
  return null;
}

export interface BridgeClient {
  baseUrl: string;
  listPanes(): Promise<PaneInfo[]>;
  spawnPane(args: {
    agent_type: string;
    cwd?: string;
    cols?: number;
    rows?: number;
    pane_id?: string;
  }): Promise<{ pane_id: string }>;
  killPane(paneId: string): Promise<void>;
  readBuffer(paneId: string, lines: number): Promise<string>;
  writeInput(paneId: string, text: string, appendNewline?: boolean): Promise<void>;
}

export function makeBridgeClient(baseUrl: string): BridgeClient {
  async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await fetch(`${baseUrl}${path}`, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!res.ok) {
      throw new Error(`bridge ${method} ${path} -> ${res.status}: ${await res.text()}`);
    }
    return (await res.json()) as T;
  }
  return {
    baseUrl,
    listPanes: () => call('GET', '/panes'),
    spawnPane: (args) => call('POST', '/panes', args),
    killPane: (id) => call('DELETE', `/panes/${encodeURIComponent(id)}`),
    readBuffer: async (id, lines) => {
      const res = await call<{ content: string }>('GET', `/panes/${encodeURIComponent(id)}/buffer?lines=${lines}`);
      return res.content;
    },
    writeInput: (id, text, appendNewline = true) =>
      call('POST', `/panes/${encodeURIComponent(id)}/input`, {
        text,
        append_newline: appendNewline,
      }),
  };
}

/**
 * Poll the bridge /panes endpoint until it responds. Used on startup to
 * discover the bridge URL (the GUI writes the port file, the bridge
 * listens — we just need to find it).
 *
 * This walks the default port range (17321–17399) trying GET /health on
 * each port. Returns the first responding base URL.
 */
export async function findBridgeUrl(): Promise<string | null> {
  for (let p = 17321; p <= 17399; p++) {
    try {
      const res = await fetch(`http://${DEFAULT_POLL_HOST}:${p}/health`, {
        signal: AbortSignal.timeout(200),
      });
      if (res.ok) {
        return `http://${DEFAULT_POLL_HOST}:${p}`;
      }
    } catch {
      /* try next */
    }
  }
  return null;
}

export type BridgeEvent =
  | { type: 'panes'; panes: PaneInfo[] }
  | { type: 'log'; entry: McpLogEntry };

/**
 * Subscribe to the bridge SSE stream. Returns an unlisten function.
 * If the bridge is unreachable, retries forever with exponential backoff.
 */
export function subscribeBridgeEvents(
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
    es.addEventListener('panes', (ev) => {
      try {
        const panes = JSON.parse((ev as MessageEvent).data) as PaneInfo[];
        onEvent({ type: 'panes', panes });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('log', (ev) => {
      try {
        const entry = JSON.parse((ev as MessageEvent).data) as McpLogEntry;
        onEvent({ type: 'log', entry });
      } catch (err) {
        onError?.(err);
      }
    });
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

export { DEFAULT_POLL_HOST, DEFAULT_POLL_INTERVAL_MS };