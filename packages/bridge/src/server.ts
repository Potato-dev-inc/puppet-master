import http from 'node:http';
import { randomUUID } from 'node:crypto';
import {
  AgentTypeSchema,
  getAgentContextProfile,
  inspectAgentModel,
  listAgentContextProfiles,
  BRIDGE_HTTP_PORT_RANGE,
  BRIDGE_PORT_FILE_ENV,
  DEFAULT_BRIDGE_HOST,
  DEFAULT_BRIDGE_PORT_FILE,
  type McpLogEntry,
  type OrchestratorChatEvent,
  type OrchestratorUserMessage,
  type PaneInfo,
  type SpawnPaneRequest,
  type WriteInputRequest,
} from '@puppet-master/shared';

export interface BridgeBackend {
  listPanes(): Promise<PaneInfo[]>;
  spawnPane(req: SpawnPaneRequest): Promise<{ pane_id: string }>;
  killPane(id: string): Promise<void>;
  readBuffer(id: string, lines: number): Promise<string>;
  writeInput(id: string, req: WriteInputRequest): Promise<void>;
  resize(id: string, cols: number, rows: number): Promise<void>;
  setProjectPath(path: string): Promise<void>;
  onLog(cb: (entry: McpLogEntry) => void): () => void;
  onPaneChange(cb: (panes: PaneInfo[]) => void): () => void;
  onOrchestratorMessage(cb: (msg: OrchestratorUserMessage) => void): () => void;
  emitChatEvent(event: OrchestratorChatEvent): void;
}

/**
 * Stub backend used during development / when the Tauri app is not running.
 * Returns fake data so the bridge can start standalone.
 */
export class StubBackend implements BridgeBackend {
  private panes = new Map<string, PaneInfo>();
  private logListeners = new Set<(e: McpLogEntry) => void>();
  private paneListeners = new Set<(p: PaneInfo[]) => void>();

  async listPanes(): Promise<PaneInfo[]> {
    return Array.from(this.panes.values());
  }

  async spawnPane(req: SpawnPaneRequest): Promise<{ pane_id: string }> {
    const id = req.pane_id ?? randomUUID();
    const info: PaneInfo = {
      id,
      agent_type: req.agent_type,
      pid: 0,
      status: 'idle',
      created_at: Date.now(),
      last_output_at: null,
      cwd: req.cwd ?? process.cwd(),
      cols: req.cols,
      rows: req.rows,
    };
    this.panes.set(id, info);
    this.emitPaneChange();
    return { pane_id: id };
  }

  async killPane(id: string): Promise<void> {
    this.panes.delete(id);
    this.emitPaneChange();
  }

  async readBuffer(_id: string, _lines: number): Promise<string> {
    return '';
  }

  async writeInput(_id: string, _req: WriteInputRequest): Promise<void> {
    /* noop */
  }

  async resize(_id: string, _cols: number, _rows: number): Promise<void> {
    /* noop */
  }

  async setProjectPath(_path: string): Promise<void> {
    /* noop */
  }

  onLog(cb: (entry: McpLogEntry) => void): () => void {
    this.logListeners.add(cb);
    return () => this.logListeners.delete(cb);
  }

  onPaneChange(cb: (panes: PaneInfo[]) => void): () => void {
    this.paneListeners.add(cb);
    return () => this.paneListeners.delete(cb);
  }

  onOrchestratorMessage(_cb: (msg: OrchestratorUserMessage) => void): () => void {
    return () => {};
  }

  emitChatEvent(_event: OrchestratorChatEvent): void {
    /* noop */
  }

  private emitPaneChange(): void {
    const snapshot = Array.from(this.panes.values());
    for (const cb of this.paneListeners) cb(snapshot);
  }
}

interface BridgeOptions {
  backend: BridgeBackend;
  host?: string;
  port?: number;
  portFile?: string;
}

export interface BridgeHandle {
  port: number;
  host: string;
  close(): Promise<void>;
  url: string;
  /** Subscribe to user messages posted from the mobile PWA. Returns unlisten fn. */
  onOrchestratorMessage(cb: (msg: OrchestratorUserMessage) => void): () => void;
  /** Push a chat event to all SSE clients (desktop + mobile). */
  emitChatEvent(event: OrchestratorChatEvent): void;
}

/**
 * Pick a free port in the Puppet Master range by trying them sequentially.
 */
async function pickPort(host: string, preferred?: number): Promise<number> {
  const net = await import('node:net');
  const tryPort = (port: number): Promise<number> =>
    new Promise((res, rej) => {
      const srv = net.createServer();
      srv.once('error', rej);
      srv.once('listening', () => srv.close(() => res(port)));
      srv.listen(port, host);
    });

  if (preferred !== undefined) {
    try {
      return await tryPort(preferred);
    } catch {
      /* fall through to range scan */
    }
  }
  for (let p = BRIDGE_HTTP_PORT_RANGE.min; p <= BRIDGE_HTTP_PORT_RANGE.max; p++) {
    try {
      return await tryPort(p);
    } catch {
      /* try next */
    }
  }
  throw new Error(
    `No free port in range ${BRIDGE_HTTP_PORT_RANGE.min}-${BRIDGE_HTTP_PORT_RANGE.max}`,
  );
}

function send(res: http.ServerResponse, status: number, body: unknown): void {
  const json = JSON.stringify(body);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(json),
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Methods': 'GET, POST, DELETE, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
  });
  res.end(json);
}

function readBody<T>(req: http.IncomingMessage): Promise<T> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      const raw = Buffer.concat(chunks).toString('utf-8');
      if (!raw) return resolve({} as T);
      try {
        resolve(JSON.parse(raw) as T);
      } catch (err) {
        reject(err);
      }
    });
    req.on('error', reject);
  });
}

/** Wrap an async handler so errors become JSON 500s. */
function wrap(
  fn: (req: http.IncomingMessage, res: http.ServerResponse, params: Record<string, string>) => Promise<void>,
): http.RequestListener {
  return async (req, res) => {
    if (req.method === 'OPTIONS') {
      res.writeHead(204, {
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, POST, DELETE, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type',
      });
      res.end();
      return;
    }
    try {
      // very small path-param parser: /panes/:id/buffer
      const url = new URL(req.url ?? '/', 'http://localhost');
      const segments = url.pathname.split('/').filter(Boolean);
      const params: Record<string, string> = {};
      // pattern: /panes/:id/buffer  or /panes/:id/input or /panes/:id/resize or /panes/:id
      if (segments[0] === 'panes' && segments[1]) {
        params.id = decodeURIComponent(segments[1]);
      }
      await fn(req, res, params);
    } catch (err) {
      send(res, 500, { error: err instanceof Error ? err.message : String(err) });
    }
  };
}

export async function startBridge(opts: BridgeOptions): Promise<BridgeHandle> {
  const host = opts.host ?? DEFAULT_BRIDGE_HOST;
  const portFile =
    opts.portFile ??
    process.env[BRIDGE_PORT_FILE_ENV] ??
    DEFAULT_BRIDGE_PORT_FILE;
  const port = await pickPort(host, opts.port);
  const backend = opts.backend;

  const sseClients = new Set<http.ServerResponse>();

  function pushSse(payload: string): void {
    for (const client of sseClients) {
      try {
        client.write(payload);
      } catch {
        sseClients.delete(client);
      }
    }
  }

  const unsubscribeLog = backend.onLog((entry) => {
    pushSse(`data: ${JSON.stringify(entry)}\n\n`);
  });

  const unsubscribePane = backend.onPaneChange((panes) => {
    pushSse(`event: panes\ndata: ${JSON.stringify(panes)}\n\n`);
  });

  const orchestratorListeners = new Set<(msg: OrchestratorUserMessage) => void>();
  const unsubscribeOrch = backend.onOrchestratorMessage((msg) => {
    for (const cb of orchestratorListeners) cb(msg);
  });

  // Desktop app subscribes here to receive mobile-sent messages
  function onOrchestratorMessage(cb: (msg: OrchestratorUserMessage) => void): () => void {
    orchestratorListeners.add(cb);
    return () => orchestratorListeners.delete(cb);
  }

  // Desktop app calls this to push chat events back to all SSE clients (mobile + itself)
  function emitChatEventToSse(event: OrchestratorChatEvent): void {
    pushSse(`event: chat\ndata: ${JSON.stringify(event)}\n\n`);
  }

  // Expose so the Tauri bridge backend can wire up after server starts
  const handle_internal = { onOrchestratorMessage, emitChatEventToSse };

  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const method = req.method ?? 'GET';
    const segments = url.pathname.split('/').filter(Boolean);

    // GET /events  — Server-Sent Events
    if (segments[0] === 'events' && method === 'GET') {
      res.writeHead(200, {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
        'Access-Control-Allow-Origin': '*',
      });
      res.write(': connected\n\n');
      sseClients.add(res);
      req.on('close', () => sseClients.delete(res));
      // initial pane snapshot
      backend.listPanes().then((panes) => {
        res.write(`event: panes\ndata: ${JSON.stringify(panes)}\n\n`);
      }).catch(() => {});
      return;
    }

    void wrap(async (rq, rs, params) => {
      const m = rq.method ?? 'GET';
      const segs = new URL(rq.url ?? '/', 'http://localhost').pathname.split('/').filter(Boolean);

      // GET /health
      if (segs[0] === 'health' && m === 'GET') {
        send(rs, 200, { ok: true, version: '0.1.1' });
        return;
      }

      // GET /agent-contexts
      if (segs[0] === 'agent-contexts' && m === 'GET') {
        send(rs, 200, listAgentContextProfiles());
        return;
      }

      // GET /agent-contexts/:agent_type
      if (segs[0] === 'agent-contexts' && segs[1] && m === 'GET') {
        const agentType = AgentTypeSchema.parse(segs[1]);
        send(rs, 200, getAgentContextProfile(agentType));
        return;
      }

      // /panes
      if (segs[0] === 'panes' && segs.length === 1) {
        if (m === 'GET') {
          const panes = await backend.listPanes();
          send(rs, 200, panes);
          return;
        }
        if (m === 'POST') {
          const body = await readBody<SpawnPaneRequest>(rq);
          const { pane_id } = await backend.spawnPane(body);
          send(rs, 201, { pane_id });
          return;
        }
      }

      // /panes/:id[/buffer|/input|/resize]
      if (segs[0] === 'panes' && segs[1]) {
        const id = params.id;
        const tail = segs[2];

        if (!tail && m === 'DELETE') {
          await backend.killPane(id);
          send(rs, 200, { ok: true });
          return;
        }
        if (tail === 'buffer' && m === 'GET') {
          const lines = Number(new URL(rq.url ?? '/', 'http://localhost').searchParams.get('lines') ?? '200');
          const content = await backend.readBuffer(id, lines);
          send(rs, 200, { content });
          return;
        }
        if (tail === 'model' && m === 'GET') {
          const panes = await backend.listPanes();
          const pane = panes.find((p) => p.id === id);
          if (!pane) {
            send(rs, 404, { error: `unknown pane: ${id}` });
            return;
          }
          const content = await backend.readBuffer(id, 200);
          send(rs, 200, inspectAgentModel(id, pane.agent_type, content));
          return;
        }
        if (tail === 'agent-context' && m === 'GET') {
          const panes = await backend.listPanes();
          const pane = panes.find((p) => p.id === id);
          if (!pane) {
            send(rs, 404, { error: `unknown pane: ${id}` });
            return;
          }
          const content = await backend.readBuffer(id, 200);
          send(rs, 200, {
            pane,
            context: getAgentContextProfile(pane.agent_type),
            model: inspectAgentModel(id, pane.agent_type, content),
            recent_buffer_preview: content.split(/\r?\n/).slice(-40).join('\n'),
          });
          return;
        }
        if (tail === 'input' && m === 'POST') {
          const body = await readBody<WriteInputRequest>(rq);
          await backend.writeInput(id, body);
          send(rs, 200, { ok: true });
          return;
        }
        if (tail === 'resize' && m === 'POST') {
          const body = await readBody<{ cols: number; rows: number }>(rq);
          await backend.resize(id, body.cols, body.rows);
          send(rs, 200, { ok: true });
          return;
        }
      }

      // /project-path
      if (segs[0] === 'project-path' && m === 'POST') {
        const body = await readBody<{ path: string }>(rq);
        await backend.setProjectPath(body.path);
        send(rs, 200, { ok: true });
        return;
      }

      // POST /orchestrator/message — mobile PWA sends user prompt to desktop orchestrator
      if (segs[0] === 'orchestrator' && segs[1] === 'message' && m === 'POST') {
        const body = await readBody<OrchestratorUserMessage>(rq);
        if (!body.text || !body.message_id) {
          send(rs, 400, { error: 'text and message_id required' });
          return;
        }
        for (const cb of orchestratorListeners) cb(body);
        send(rs, 200, { ok: true });
        return;
      }

      send(rs, 404, { error: 'not found', path: rq.url });
    })(req, res);
  });

  await new Promise<void>((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(port, host, () => resolveListen());
  });

  // write port file so external MCP clients can find us
  const { writeFile } = await import('node:fs/promises');
  try {
    await writeFile(portFile, `${host}:${port}\n`, 'utf-8');
    process.stderr.write(`[bridge] port file written: ${portFile}\n`);
  } catch (err) {
    process.stderr.write(`[bridge] port file write FAILED: ${portFile}: ${err}\n`);
    throw err;
  }

  const handle: BridgeHandle = {
    host,
    port,
    url: `http://${host}:${port}`,
    onOrchestratorMessage: handle_internal.onOrchestratorMessage,
    emitChatEvent: handle_internal.emitChatEventToSse,
    async close() {
      unsubscribeLog();
      unsubscribePane();
      unsubscribeOrch();
      orchestratorListeners.clear();
      for (const client of sseClients) {
        try { client.end(); } catch { /* ignore */ }
      }
      sseClients.clear();
      await new Promise<void>((res, rej) => server.close((err) => err ? rej(err) : res()));
      const { unlink } = await import('node:fs/promises');
      try { await unlink(portFile); } catch { /* ignore */ }
    },
  };

  return handle;
}

// Allow `node dist/server.js` for standalone dev
if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}`) {
  const handle = await startBridge({ backend: new StubBackend() });
  // eslint-disable-next-line no-console
  console.error(`[bridge] listening on ${handle.url}`);
}
