// HTTP client for the Puppet Master bridge (Rust side, default 127.0.0.1:17321): URL discovery and
// the typed BridgeClient. Wire types live in ./bridge-types, SSE subscription in ./bridge-sse.
import type { AgentContextProfile } from '@puppet-master/shared';
import { ngrokRequestHeaders } from './bridge-ngrok';
import { decodeBase64 } from './bridge-sse';
import type { BridgeClient, PaneViewportResponse } from './bridge-types';
import type { Question } from './questions';
import { mergeBridgeHeaders } from './mobile-pairing-auth';
import type { PreviewUrl } from './pane-preview';

export type * from './bridge-types';
export { subscribeBridgeEvents } from './bridge-sse';

const DEFAULT_POLL_HOST = '127.0.0.1';
const DEFAULT_POLL_INTERVAL_MS = 200;

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
    listPreviews: async () => (await call<{ previews: Record<string, PreviewUrl[]> }>('GET', '/previews')).previews ?? {},
    spawnPane: async (args) => {
      // The route answers `{ ok, snapshot: { pane_id }, suggested_wait }` (MCP hint envelope).
      const res = await call<{ pane_id?: string; snapshot?: { pane_id?: string } }>('POST', '/panes', args);
      const paneId = res.snapshot?.pane_id ?? res.pane_id;
      if (!paneId) throw new Error('spawn returned no pane id');
      return { pane_id: paneId };
    },
    killPane: (id) => call('DELETE', `/panes/${encodeURIComponent(id)}`),
    stopAllPanes: () => call('POST', '/panes/stop-all', {}),
    listQuestions: async () => (await call<{ questions: Question[] }>('GET', '/questions')).questions,
    answerQuestion: (id, answer) => call('POST', `/questions/${encodeURIComponent(id)}/answer`, answer),
    dismissQuestion: (id) => call('POST', `/questions/${encodeURIComponent(id)}/dismiss`, {}),
    setPaneViewport: (id, view) =>
      call<PaneViewportResponse>('POST', `/panes/${encodeURIComponent(id)}/viewport`, view),
    listAgentTypes: () => call('GET', '/agents/types'),
    digest: (opts) => {
      const q = new URLSearchParams();
      if (opts?.since !== undefined) q.set('since', String(opts.since));
      if (opts?.seen) q.set('seen', '1');
      const qs = q.toString();
      return call('GET', `/digest${qs ? `?${qs}` : ''}`);
    },
    readBuffer: async (id, lines) => {
      const res = await call<{ content: string }>('GET', `/panes/${encodeURIComponent(id)}/buffer?lines=${lines}`);
      return res.content;
    },
    readRawBuffer: async (id, lines) => {
      const res = await call<{ data: number[] }>('GET', `/panes/${encodeURIComponent(id)}/raw?lines=${lines}`);
      return res.data;
    },
    readScreen: async (id) => {
      const res = await call<{ b64: string; cols: number; rows: number }>(
        'GET',
        `/panes/${encodeURIComponent(id)}/screen`,
      );
      return { data: decodeBase64(res.b64), cols: res.cols, rows: res.rows };
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
    switchAgentModel: (id, args) =>
      call('POST', `/panes/${encodeURIComponent(id)}/model`, args),
    writeInput: (id, text, appendNewline = true, options) =>
      call('POST', `/panes/${encodeURIComponent(id)}/input`, {
        text,
        append_newline: appendNewline,
        via_opencode_api: options?.viaOpencodeApi ?? false,
        ...(options?.modelProvider ? { model_provider: options.modelProvider } : {}),
        ...(options?.modelId ? { model_id: options.modelId } : {}),
      }),
    resize: (id, cols, rows) =>
      call('POST', `/panes/${encodeURIComponent(id)}/resize`, { cols, rows }),
    getWorkspaceState: () => call('GET', '/workspace/state'),
    listMcpTools: () => call('GET', '/mcp/tools'),
    listMcpResources: () => call('GET', '/mcp/resources'),
    listMcpPrompts: () => call('GET', '/mcp/prompts'),
    listTasks: () => call('GET', '/tasks'),
    createTask: (args) => call('POST', '/tasks', args),
    claimTask: (taskId, args) => call('POST', `/tasks/${encodeURIComponent(taskId)}/claim`, args),
    patchTaskStatus: (taskId, args) => call('POST', `/tasks/${encodeURIComponent(taskId)}/status`, args),
    completeTask: (taskId, args) => call('POST', `/tasks/${encodeURIComponent(taskId)}/complete`, args),
    blockTask: (taskId, args) => call('POST', `/tasks/${encodeURIComponent(taskId)}/block`, args),
    listLocks: () => call('GET', '/locks'),
    acquireResourceLock: (args) => call('POST', '/locks', args),
    releaseResourceLock: (args) => call('POST', '/locks/release', args),
    getAudit: () => call('GET', '/audit'),
    readSessionContext: () => call('GET', '/session/context'),
    updateSessionContext: (patch) => call('PATCH', '/session/context', patch),
    setPaneRole: (paneId, role) =>
      call('POST', `/panes/${encodeURIComponent(paneId)}/role`, { role }),
    readPaneDigest: (paneId) =>
      call('GET', `/panes/${encodeURIComponent(paneId)}/digest`),
    updatePaneDigest: (args) =>
      call('POST', `/panes/${encodeURIComponent(args.pane_id)}/digest`, args),
    delegateTask: (args) => call('POST', '/delegate-task', args),
    readOrchestratorState: () => call('GET', '/orchestrator/state'),
    updateOrchestratorState: (patch) => call('PATCH', '/orchestrator/state', patch),
    buildContextPack: (args) => call('POST', '/context-packs', args),
    readProjectIrStatus: () => call('GET', '/project-ir/status'),
    readLibrarianPrompt: () => call('GET', '/librarian/prompt'),
    waitForPanes: (args) => call('POST', '/panes/wait', args),
    waitForModel: (args) => call('POST', '/panes/wait/model', args),
    waitForTask: (args) => call('POST', '/panes/wait/task', args),
    readRecentEvents: (args = {}) => {
      const params = new URLSearchParams();
      if (args.limit != null) params.set('limit', String(args.limit));
      if (args.pane_id) params.set('pane_id', args.pane_id);
      if (args.since_id) params.set('since_id', args.since_id);
      if (args.types?.length) params.set('types', args.types.join(','));
      const query = params.toString();
      return call('GET', `/events/recent${query ? `?${query}` : ''}`);
    },
    readOpencodeWorkerStatus: (paneId) =>
      call('GET', `/panes/${encodeURIComponent(paneId)}/opencode/status`),
    readOpencodeMessages: (paneId, args) => {
      const params = new URLSearchParams();
      if (args?.limit != null) params.set('limit', String(args.limit));
      if (args?.role) params.set('role', args.role);
      const query = params.toString();
      return call(
        'GET',
        `/panes/${encodeURIComponent(paneId)}/opencode/messages${query ? `?${query}` : ''}`,
      );
    },
    replyOpencodePermission: (paneId, requestId, reply) =>
      call('POST', `/panes/${encodeURIComponent(paneId)}/opencode/permissions/${encodeURIComponent(requestId)}/reply`, {
        reply,
      }),
    replyOpencodeQuestion: (paneId, args) =>
      call('POST', `/panes/${encodeURIComponent(paneId)}/opencode/question/reply`, args),
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

export { DEFAULT_POLL_HOST, DEFAULT_POLL_INTERVAL_MS };
