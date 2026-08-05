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

export interface WriteInputOptions {
  viaOpencodeApi?: boolean;
  modelProvider?: string;
  modelId?: string;
}

export interface PaneStateProjection {
  pane_id: string;
  agent_type: string | null;
  pid: number | null;
  cwd: string | null;
  role: PaneRole | null;
  status: string;
  input_events: number;
  output_events: number;
  killed: boolean;
}

export interface WorkspaceStateProjection {
  panes: PaneStateProjection[];
  task_count: number;
  lock_count: number;
}

export interface TaskProjection {
  id: string;
  title: string;
  status: string;
  exclusive: boolean;
  claimed_by: string | null;
  lease_expires_at_ms: number | null;
  reviewer_id: string | null;
  evidence: string | null;
  blocked_reason: string | null;
}

export interface LockProjection {
  resource_id: string;
  resource_type: string;
  owner: string;
  lease_expires_at_ms: number | null;
}

export interface AuditEntryProjection {
  event_id: string;
  timestamp_ms: number;
  actor: string;
  event_type: string;
}

export type PaneRole = 'implementer' | 'reviewer' | 'shell' | 'orchestrator' | 'observer';

export interface PaneDigest {
  pane_id: string;
  summary: string;
  source: string;
  updated_at_ms: number;
}

export interface OpenCodeWorkerStatus {
  pane_id: string;
  pane_status: string;
  serve_healthy: boolean;
  session_id: string;
  pending_permission_count: number;
  pending_permission_ids: string[];
  active_key_profile: string | null;
  key_swap_pending: boolean;
  key_swap_kind?: string | null;
  from_profile?: string | null;
  to_profile?: string | null;
}

export interface OpenCodeWaitSnapshot {
  serve_healthy: boolean;
  pending_permission_count: number;
  pending_permission_ids: string[];
  pending_key_swap?: {
    kind: string;
    from_profile: string;
    to_profile?: string | null;
  } | null;
  session_model?: { providerID: string; modelID: string } | null;
  last_user_model?: { providerID: string; modelID: string } | null;
  tui_attached?: boolean;
  reattaching?: boolean;
}

export interface WaitForPanesResult {
  reason: string;
  pane_id: string;
  status: string | null;
  opencode: OpenCodeWaitSnapshot | null;
  from_profile?: string | null;
  to_profile?: string | null;
  task_id?: string | null;
  task_status?: string | null;
}

export interface SuggestedWait {
  tool: string;
  args: Record<string, unknown>;
}

export interface MutateToolResult {
  ok: boolean;
  snapshot?: unknown;
  suggested_wait?: SuggestedWait;
  pane_id?: string;
  written?: boolean;
  task_id?: string | null;
  target_pane_id?: string | null;
  prompt?: string;
}

export interface OpenCodeWorkerEvent {
  pane_id: string;
  event: string;
  pane_status: string;
  serve_healthy: boolean;
  pending_permission_count: number;
  pending_permission_ids: string[];
  from_profile?: string | null;
  to_profile?: string | null;
  reason?: string | null;
  auto_rotated?: boolean | null;
}

export interface SessionTimelineEvent {
  timestamp_ms: number;
  actor: string;
  event_type: string;
  summary: string;
}

export interface LockConflictProjection {
  resource_id: string;
  requested_owner_id: string;
  existing_owner_id: string;
  timestamp_ms: number;
}

export interface OrchestratorStateProjection {
  standby_poll_ms: number;
  standby_max_ms: number;
}

export interface SessionContextProjection {
  current_goal: string | null;
  pane_roles: Record<string, PaneRole>;
  pane_digests: Record<string, PaneDigest>;
  timeline: SessionTimelineEvent[];
  lock_conflicts: LockConflictProjection[];
  orchestrator: OrchestratorStateProjection;
}

export interface DelegateTaskRequest {
  task_id?: string;
  target_pane_id?: string;
  intent: string;
  acceptance_criteria: string[];
  locked_resources?: string[];
  evidence_required?: string[];
  token_budget_hint?: number;
  timeout_ms?: number;
}

export interface DelegateTaskResponse {
  ok: boolean;
  task_id?: string | null;
  target_pane_id?: string | null;
  prompt: string;
}

export interface McpRegistryTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  outputSchema?: Record<string, unknown>;
  safety: 'read_only' | 'mutating' | 'destructive';
  visibility: { sidebar: boolean; external_mcp: boolean };
  method: string;
  path: string;
}

export interface McpRegistryResource {
  uri: string;
  name: string;
  description: string;
  mimeType: string;
}

export interface McpRegistryPrompt {
  name: string;
  description: string;
  arguments: Array<{ name: string; description: string; required: boolean }>;
}

export interface ContextPackRequest {
  task_id?: string;
  agent_id?: string;
  user_constraints?: string[];
  manager_instructions?: string;
  raw_scrollback?: string;
}

export interface ContextPack {
  prompt: string;
  expected_report_format: string[];
  allowed_tools: string[];
  ownership_boundaries: string[];
  evidence_requirements: string[];
  estimated_raw_scrollback_bytes: number;
  context_pack_bytes: number;
  project_ir_included: boolean;
  project_ir_stale: boolean;
  project_ir_indexer_command: string;
}

export interface LibrarianPromptResponse {
  prompt: string;
  delegate_to: string;
  completion_marker: string;
}

export interface ProjectIrStatus {
  ir_exists: boolean;
  git_sha: string | null;
  indexed_git_sha: string | null;
  stale: boolean;
  generated_at_ms: number | null;
  indexer_command: string;
}

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
  switchAgentModel(
    paneId: string,
    args: { model_id: string; model_provider?: string },
  ): Promise<{ ok: boolean; provider_id: string; model_id: string; tui_synced?: boolean }>;
  writeInput(paneId: string, text: string, appendNewline?: boolean, options?: WriteInputOptions): Promise<void>;
  resize(paneId: string, cols: number, rows: number): Promise<void>;
  getWorkspaceState(): Promise<WorkspaceStateProjection>;
  listMcpTools(): Promise<McpRegistryTool[]>;
  listMcpResources(): Promise<McpRegistryResource[]>;
  listMcpPrompts(): Promise<McpRegistryPrompt[]>;
  listTasks(): Promise<TaskProjection[]>;
  createTask(args: { title: string; exclusive?: boolean }): Promise<{ task_id: string }>;
  claimTask(taskId: string, args: { agent_id: string; lease_ms?: number }): Promise<unknown>;
  patchTaskStatus(taskId: string, args: { status: string }): Promise<unknown>;
  completeTask(taskId: string, args: { agent_id: string; evidence?: string }): Promise<unknown>;
  blockTask(taskId: string, args: { agent_id: string; reason: string }): Promise<unknown>;
  listLocks(): Promise<LockProjection[]>;
  acquireResourceLock(args: {
    resource_type: string;
    name: string;
    owner_id: string;
    lease_ms?: number;
  }): Promise<{ resource_id: string; locked: boolean }>;
  releaseResourceLock(args: {
    resource_type: string;
    name: string;
    owner_id: string;
  }): Promise<unknown>;
  getAudit(): Promise<AuditEntryProjection[]>;
  readSessionContext(): Promise<SessionContextProjection>;
  updateSessionContext(patch: { current_goal?: string | null }): Promise<SessionContextProjection>;
  setPaneRole(paneId: string, role: PaneRole): Promise<SessionContextProjection>;
  readPaneDigest(paneId: string): Promise<PaneDigest>;
  updatePaneDigest(args: { pane_id: string; summary: string; source?: string }): Promise<PaneDigest>;
  delegateTask(args: DelegateTaskRequest): Promise<DelegateTaskResponse>;
  readOrchestratorState(): Promise<OrchestratorStateProjection>;
  updateOrchestratorState(patch: Partial<OrchestratorStateProjection>): Promise<OrchestratorStateProjection>;
  buildContextPack(args: ContextPackRequest): Promise<ContextPack>;
  readProjectIrStatus(): Promise<ProjectIrStatus>;
  readLibrarianPrompt(): Promise<LibrarianPromptResponse>;
  waitForPanes(args: {
    pane_ids: string[];
    until?: string[];
    timeout_ms?: number;
    task_id?: string;
    output_regex?: string;
    match?: { provider_id?: string; model_id?: string };
  }): Promise<WaitForPanesResult>;
  waitForModel(args: {
    pane_id: string;
    provider_id?: string;
    model_id?: string;
    timeout_ms?: number;
  }): Promise<WaitForPanesResult>;
  waitForTask(args: {
    pane_id: string;
    task_id: string;
    until?: string[];
    timeout_ms?: number;
  }): Promise<WaitForPanesResult>;
  readRecentEvents(args?: {
    limit?: number;
    pane_id?: string;
    types?: string[];
    since_id?: string;
  }): Promise<unknown[]>;
  readOpencodeWorkerStatus(paneId: string): Promise<OpenCodeWorkerStatus>;
  readOpencodeMessages(
    paneId: string,
    args?: { limit?: number; role?: 'all' | 'user' | 'assistant' },
  ): Promise<unknown>;
  replyOpencodePermission(paneId: string, requestId: string, reply: string): Promise<void>;
  replyOpencodeQuestion(
    paneId: string,
    args: { answer: string; request_id?: string },
  ): Promise<unknown>;
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
  | { type: 'opencode-worker'; event: OpenCodeWorkerEvent }
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
    es.addEventListener('opencode-worker', (ev) => {
      try {
        const event = JSON.parse((ev as MessageEvent).data) as import('./bridge').OpenCodeWorkerEvent;
        onEvent({ type: 'opencode-worker', event });
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
