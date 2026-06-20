import type {
  AgentContextProfile,
  AgentModelInspection,
  McpLogEntry,
  OrchestratorChatEvent,
  PaneInfo,
} from '@puppet-master/shared';
import type { PublicSettings } from './bridge-settings';
import { isNgrokHost, ngrokRequestHeaders } from './bridge-ngrok';
import { mergeBridgeHeaders } from './mobile-pairing-auth';
import { subscribeBridgeEventsViaFetch } from './bridge-sse';

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
  readRawBuffer(paneId: string, lines: number): Promise<number[]>;
  readSnapshot(paneId: string): Promise<string>;
  listAgentContexts(): Promise<AgentContextProfile[]>;
  readAgentContext(args: { agent_type?: string; pane_id?: string }): Promise<unknown>;
  inspectAgentModel(paneId: string, lines?: number): Promise<AgentModelInspection>;
  writeInput(paneId: string, text: string, appendNewline?: boolean): Promise<void>;
  resize(paneId: string, cols: number, rows: number): Promise<void>;
  getSettings(): Promise<PublicSettings>;
  patchSettings(patch: Partial<PublicSettings>): Promise<PublicSettings>;
  postOrchestratorMessage(text: string, messageId: string): Promise<void>;
  postOrchestratorViewport(viewport: {
    width: number;
    height: number;
    active: boolean;
  }): Promise<void>;
}

export function makeBridgeClient(baseUrl: string): BridgeClient {
  async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers = mergeBridgeHeaders({ ...ngrokRequestHeaders(baseUrl) });
    if (body) headers['Content-Type'] = 'application/json';
    const res = await fetch(`${baseUrl}${path}`, {
      method,
      headers: Object.keys(headers).length > 0 ? headers : undefined,
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
    readRawBuffer: async (id, lines) => {
      const res = await call<{ data: number[] }>('GET', `/panes/${encodeURIComponent(id)}/raw?lines=${lines}`);
      return res.data;
    },
    readSnapshot: async (id) => {
      const res = await call<{ content: string }>('GET', `/panes/${encodeURIComponent(id)}/snapshot`);
      return res.content;
    },
    listAgentContexts: () => call('GET', '/agent-contexts'),
    readAgentContext: async (args) => {
      if (args.pane_id) {
        return call('GET', `/panes/${encodeURIComponent(args.pane_id)}/agent-context`);
      }
      const contexts = await call<Array<AgentContextProfile & { agent_type: string }>>('GET', '/agent-contexts');
      const context = contexts.find((candidate) => candidate.agent_type === args.agent_type);
      if (!context) throw new Error(`unknown agent_type: ${args.agent_type}`);
      return context;
    },
    inspectAgentModel: (id, lines = 200) =>
      call('GET', `/panes/${encodeURIComponent(id)}/model?lines=${lines}`),
    writeInput: (id, text, appendNewline = true) =>
      call('POST', `/panes/${encodeURIComponent(id)}/input`, {
        text,
        append_newline: appendNewline,
      }),
    resize: (id, cols, rows) =>
      call('POST', `/panes/${encodeURIComponent(id)}/resize`, { cols, rows }),
    getSettings: () => call('GET', '/settings'),
    patchSettings: (patch) => call('PATCH', '/settings', patch),
    postOrchestratorMessage: (text, messageId) =>
      call('POST', '/orchestrator/message', { text, message_id: messageId }),
    postOrchestratorViewport: (viewport) =>
      call<void>('POST', '/orchestrator/viewport', viewport),
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

function shouldUseFetchBridgeSse(baseUrl: string): boolean {
  if (isNgrokHost(baseUrl)) return true;
  try {
    const { hostname } = new URL(baseUrl);
    return hostname !== '127.0.0.1' && hostname !== 'localhost';
  } catch {
    return false;
  }
}

export type BridgeEvent =
  | { type: 'panes'; panes: PaneInfo[] }
  | { type: 'log'; entry: McpLogEntry }
  | { type: 'chat'; event: OrchestratorChatEvent }
  | { type: 'terminal'; pane_id: string; data: number[] }
  | { type: 'terminal-snapshot'; pane_id: string; snapshot: string }
  | { type: 'pane-status'; pane_id: string; status: PaneInfo['status'] }
  | { type: 'pane-resize'; pane_id: string; cols: number; rows: number }
  | { type: 'settings'; settings: PublicSettings }
  | { type: 'orchestrator-viewport'; width: number; height: number; active: boolean };

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
    return subscribeBridgeEventsViaFetch(baseUrl, onEvent, onError);
  }

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
    es.addEventListener('chat', (ev) => {
      try {
        const event = JSON.parse((ev as MessageEvent).data) as OrchestratorChatEvent;
        onEvent({ type: 'chat', event });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('terminal', (ev) => {
      try {
        const payload = JSON.parse((ev as MessageEvent).data) as { pane_id: string; data: number[] };
        onEvent({ type: 'terminal', pane_id: payload.pane_id, data: payload.data });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('terminal-snapshot', (ev) => {
      try {
        const payload = JSON.parse((ev as MessageEvent).data) as { pane_id: string; snapshot: string };
        onEvent({ type: 'terminal-snapshot', pane_id: payload.pane_id, snapshot: payload.snapshot });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('pane-status', (ev) => {
      try {
        const payload = JSON.parse((ev as MessageEvent).data) as {
          pane_id: string;
          status: PaneInfo['status'];
        };
        onEvent({ type: 'pane-status', pane_id: payload.pane_id, status: payload.status });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('pane-resize', (ev) => {
      try {
        const payload = JSON.parse((ev as MessageEvent).data) as {
          pane_id: string;
          cols: number;
          rows: number;
        };
        onEvent({
          type: 'pane-resize',
          pane_id: payload.pane_id,
          cols: payload.cols,
          rows: payload.rows,
        });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('settings', (ev) => {
      try {
        const settings = JSON.parse((ev as MessageEvent).data) as PublicSettings;
        onEvent({ type: 'settings', settings });
      } catch (err) {
        onError?.(err);
      }
    });
    es.addEventListener('orchestrator-viewport', (ev) => {
      try {
        const payload = JSON.parse((ev as MessageEvent).data) as {
          width: number;
          height: number;
          active: boolean;
        };
        onEvent({
          type: 'orchestrator-viewport',
          width: payload.width,
          height: payload.height,
          active: payload.active,
        });
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
