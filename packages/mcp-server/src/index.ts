#!/usr/bin/env node
/**
 * Puppet Master MCP server.
 *
 * Speaks stdio JSON-RPC using the `@modelcontextprotocol/sdk`. Each tool
 * call proxies to the local HTTP bridge (which fronts the Rust PTY manager
 * inside the running Tauri app).
 *
 * CRITICAL: every log line must go to stderr — never stdout — otherwise
 * we corrupt the JSON-RPC stream.
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { randomUUID } from 'node:crypto';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import {
  AgentTypeSchema,
  assertWorkerPaneTarget,
  findReusableWorkerPane,
  formatPaneListForOrchestrator,
  PaneInfoSchema,
  SpawnPaneRequestSchema,
  SwitchModelRequestSchema,
  WriteInputRequestSchema,
} from '@puppet-master/shared';
import { readBridgePort } from '@puppet-master/shared/bridge-port';

const log = (...args: unknown[]) => {
  process.stderr.write(`[puppet-master-mcp] ${args.map(String).join(' ')}\n`);
};

const DEFAULT_AGENT_WAIT_MS = 120_000;
const MCP_INSTRUCTIONS =
  'Puppet Master MCP (agent mode). Sequence: list_agents → run_agent(worker_id or pane_id, task) → handle; send_message(handle) steers the current turn; followup_task(handle) continues the conversation; wait_agents(handles) collects answers. Run watch_command in the background after run/followup/delegate or WAIT_TIMEOUT. Reuse idempotency_key after WAIT_TIMEOUT. Do not close the user pane as cleanup.';


interface BridgeClient {
  baseUrl: string;
  sessionId: string;
}

interface RegistryTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  visibility?: { external_mcp?: boolean };
  method?: string;
  path?: string;
  outputSchema?: Record<string, unknown>;
}

interface BridgeToolError { code: string; message: string; recoverable: boolean; retry_after_ms?: number; context?: unknown }

class ToolCallError extends Error {
  constructor(readonly detail: BridgeToolError) { super(detail.message); }
}

function waitTimeoutMs(args: Record<string, unknown>): number {
  const timeout = typeof args.timeout_ms === 'number' ? args.timeout_ms : 120_000;
  return Math.min(timeout + 15_000, 310_000);
}

function ensureIdempotencyKey(args: Record<string, unknown>): Record<string, unknown> {
  if (typeof args.idempotency_key === 'string' && args.idempotency_key.trim().length > 0) {
    return args;
  }
  return { ...args, idempotency_key: randomUUID() };
}

function toolTimeoutMs(path: string, args: Record<string, unknown>): number {
  if (path === '/agents/run') {
    if (args.background === true) return 30_000;
    return Math.min((typeof args.wait_ms === 'number' ? args.wait_ms : DEFAULT_AGENT_WAIT_MS) + 15_000, 320_000);
  }
  if (path === '/agents/followup') {
    return Math.min((typeof args.wait_ms === 'number' ? args.wait_ms : 120_000) + 15_000, 320_000);
  }
  if (
    path === '/agents/wait'
    || path.includes('/panes/wait')
    || (path.includes('/operations/') && path.endsWith('/wait'))
  ) {
    return waitTimeoutMs(args);
  }
  return 30_000;
}

async function waitTimeoutError(
  client: BridgeClient,
  path: string,
  args: Record<string, unknown>,
  err: unknown,
): Promise<unknown> {
  const timedOut = err instanceof Error && (err.name === 'AbortError' || /timed out|10060/i.test(err.message));
  if (!timedOut) return err;
  const key = typeof args.idempotency_key === 'string' ? args.idempotency_key : undefined;
  if (key) {
    try {
      const query = new URLSearchParams({ idempotency_key: key });
      if (typeof args.project_path === 'string') query.set('project_path', args.project_path);
      const snap = await call<Record<string, unknown>>(client, 'GET', `/operations/by-key?${query.toString()}`);
      const status = String(snap.status ?? '');
      const handle = String(snap.agent_run_id ?? snap.operation_id ?? args.handle ?? '');
      const outcome =
        status === 'completed' || status === 'failed' || status === 'cancelled'
          ? 'completed_delivery_failed'
          : ['queued', 'starting', 'running', 'waiting_input'].includes(status)
            ? 'pending'
            : 'not_dispatched';
      return new ToolCallError({
        code: 'WAIT_TIMEOUT',
        message: 'transport timed out; use next_action instead of retrying with a new idempotency key',
        recoverable: true,
        context: {
          outcome,
          handle,
          operation_id: snap.operation_id,
          turn_id: `turn-${String(snap.turn_index ?? 0)}`,
          idempotency_key: key,
          path,
          result: outcome === 'completed_delivery_failed' ? snap.result : undefined,
          next_action: {
            tool: 'wait_agents',
            handles: handle ? [handle] : [],
            until: ['completed', 'failed', 'cancelled', 'waiting_input'],
          },
        },
      });
    } catch {
      /* fall through to handle-based recovery */
    }
  }
  const handle = [args.handle, args.worker_id, args.name].find(
    (value): value is string => typeof value === 'string' && value.length > 0,
  );
  if (handle && (path === '/agents/followup' || path === '/agents/run')) {
    return new ToolCallError({
      code: 'WAIT_TIMEOUT',
      message: `transport timed out; call wait_agents with handle ${handle} instead of retrying the same task`,
      recoverable: true,
      context: {
        outcome: 'unknown',
        handle,
        path,
        suggestion: 'wait_agents',
      },
    });
  }
  return err;
}

function resolveRegistryPath(template: string, args: Record<string, unknown>): string {
  return template.replace(/\{([^}]+)\}/g, (_match, key: string) => {
    const value = args[key];
    if (typeof value !== 'string' || value.length === 0) {
      throw new Error(`missing path parameter: ${key}`);
    }
    return encodeURIComponent(value);
  });
}

function appendQuery(path: string, args: Record<string, unknown>, keys: string[]): string {
  const params = new URLSearchParams();
  for (const key of keys) {
    const value = args[key];
    if (value === undefined || value === null) continue;
    if (key === 'types' && Array.isArray(value)) {
      for (const item of value) params.append('types', String(item));
      continue;
    }
    params.set(key, String(value));
  }
  const qs = params.toString();
  return qs ? `${path}?${qs}` : path;
}

async function callWithTimeout<T>(
  client: BridgeClient,
  method: string,
  path: string,
  body: unknown,
  timeoutMs: number,
  externalSignal?: AbortSignal,
): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const abort = () => controller.abort();
  externalSignal?.addEventListener('abort', abort, { once: true });
  try {
    const res = await fetch(url, {
      method,
      headers: { 'X-Puppet-Master-Session': client.sessionId, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    if (!res.ok) {
      const text = await res.text();
      throw bridgeHttpToolError(res.status, text, `${method} ${path}`);
    }
    return (await res.json()) as T;
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      throw new Error(`bridge ${method} ${path} timed out after ${timeoutMs}ms`);
    }
    throw err;
  } finally {
    clearTimeout(timer);
    externalSignal?.removeEventListener('abort', abort);
  }
}

async function waitForOperationWithProgress(client: BridgeClient, path: string, body: Record<string, unknown>, timeoutMs: number, signal: AbortSignal, onProgress: (snapshot: unknown) => Promise<void>): Promise<unknown> {
  const deadline = Date.now() + timeoutMs;
  let revision = typeof body.after_revision === 'number' ? body.after_revision : -1;
  const until = Array.isArray(body.until) ? body.until.filter((item): item is string => typeof item === 'string') : undefined;
  const waitBody = Object.fromEntries(Object.entries(body).filter(([key]) => key !== 'until'));
  let lastResponse: { snapshot?: Record<string, unknown>; reason?: string } | undefined;
  while (Date.now() < deadline) {
    if (signal.aborted) throw new Error('request cancelled');
    const remainingMs = deadline - Date.now();
    const response = await callWithTimeout<{ snapshot?: Record<string, unknown>; reason?: string }>(client, 'POST', path, { ...waitBody, after_revision: revision, timeout_ms: Math.min(4_000, remainingMs) }, Math.min(20_000, remainingMs + 1_000), signal);
    lastResponse = response;
    const snapshot: Record<string, unknown> = response.snapshot ?? response as Record<string, unknown>;
    const nextRevision = typeof snapshot.revision === 'number' ? snapshot.revision : revision;
    const status = String(snapshot.status ?? '');
    if (nextRevision !== revision || ['completed', 'failed', 'cancelled'].includes(status)) await onProgress(snapshot);
    revision = nextRevision;
    if (['completed', 'failed', 'cancelled'].includes(status) || response.reason === 'matched_state') return response;
    if (until && until.includes(status)) return { ...response, reason: 'matched_state' };
  }
  if (lastResponse) return { snapshot: lastResponse.snapshot ?? lastResponse, reason: 'timeout' };
  const first = await callWithTimeout<{ snapshot?: Record<string, unknown> }>(client, 'POST', path, { ...waitBody, after_revision: revision, timeout_ms: 1 }, 5_000, signal);
  return { snapshot: first.snapshot ?? first, reason: 'timeout' };
}

async function invokeRegistryTool(
  clientRef: { current: BridgeClient },
  name: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const tools = await readRegistryTools(clientRef).catch(() => FALLBACK_AGENT_TOOLS);
  const def = tools.find((tool) => tool.name === name) ?? FALLBACK_AGENT_TOOLS.find((tool) => tool.name === name);
  if (!def?.method || !def.path) {
    throw new Error(`unknown tool: ${name}`);
  }

  let path = def.path;
  if (def.method === 'GET') {
    if (name === 'read_recent_events') {
      path = appendQuery(path, args, ['limit', 'pane_id', 'since_id', 'types']);
    } else if (name === 'inspect_agent_model') {
      path = appendQuery(resolveRegistryPath(path, args), args, ['lines']);
    } else if (name === 'read_opencode_messages') {
      path = appendQuery(resolveRegistryPath(path, args), args, ['limit', 'role']);
    } else if (name === 'agent_transcript') {
      path = appendQuery(resolveRegistryPath(path, args), args, ['project_path', 'operation_id', 'after']);
    } else if (name === 'list_agents' || name === 'list_workers') {
      path = appendQuery(path, args, ['project_path']);
    } else if (name === 'read_opencode_worker_status') {
      path = appendQuery(path, args, ['pane_id', 'worker_id']);
    } else if (path.includes('{')) {
      path = resolveRegistryPath(path, args);
      path = appendQuery(path, args, ['lines']);
    }
  } else {
    path = path.includes('{') ? resolveRegistryPath(path, args) : path;
  }

  const timeoutMs = toolTimeoutMs(path, args);
  const body =
    def.method === 'GET'
      ? undefined
      : path === '/agents/run' || path === '/agents/followup' || path === '/agents/send'
        ? ensureIdempotencyKey(args)
        : args;
  try {
    return await callWithTimeout(clientRef.current, def.method, path, body, timeoutMs);
  } catch (err) {
    throw await waitTimeoutError(clientRef.current, path, (body ?? args) as Record<string, unknown>, err);
  }
}

async function makeClient(sessionId: string): Promise<BridgeClient> {
  const { host, port } = await readBridgePort();
  return { baseUrl: `http://${host}:${port}`, sessionId };
}

async function call<T>(client: BridgeClient, method: string, path: string, body?: unknown): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  let res: Response;
  try {
    res = await fetch(url, {
      method,
      headers: { 'X-Puppet-Master-Session': client.sessionId, ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch (err) {
    throw new Error(`bridge ${method} ${path} -> fetch failed at ${client.baseUrl}: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (!res.ok) {
    const text = await res.text();
    throw bridgeHttpToolError(res.status, text, `${method} ${path}`);
  }
  return (await res.json()) as T;
}

async function callWithRefresh<T>(
  clientRef: { current: BridgeClient },
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  try {
    return await call<T>(clientRef.current, method, path, body);
  } catch (err) {
    if (method !== 'GET') throw err;
    const message = err instanceof Error ? err.message : String(err);
    if (!message.includes('fetch failed')) throw err;
    const next = await makeClient(clientRef.current.sessionId);
    clientRef.current = next;
    log('refreshed bridge client', next.baseUrl);
    return await call<T>(clientRef.current, method, path, body);
  }
}

const FALLBACK_AGENT_TOOLS: RegistryTool[] = [
  { name: 'set_mode', description: 'Choose agent, shell, or both for this MCP connection.', inputSchema: { type: 'object', properties: { mode: { type: 'string', enum: ['agent', 'shell', 'both'] } }, required: ['mode'] }, method: 'POST', path: '/mcp/mode', visibility: { external_mcp: true } },
  { name: 'run_agent', description: 'Start or adopt a worker with a task and return a stable handle. Pass worker_id from list_workers to adopt a UI pane; the pane directory is used automatically.', inputSchema: { type: 'object', properties: { task: { type: 'string' }, name: { type: 'string' }, handle: { type: 'string' }, worker_id: { type: 'string' }, project_path: { type: 'string' }, context_mode: { type: 'string' }, model: { type: 'string' }, agent_type: { type: 'string' }, background: { type: 'boolean' }, wait_ms: { type: 'integer' } }, required: ['task'] }, method: 'POST', path: '/agents/run', visibility: { external_mcp: true } },
  { name: 'wait_agents', description: 'Wait for worker handles to report a meaningful change and return the turn result.', inputSchema: { type: 'object', properties: { handles: { type: 'array', items: { type: 'string' } }, after_cursor: { type: 'integer' }, timeout_ms: { type: 'integer' } }, required: ['handles'] }, method: 'POST', path: '/agents/wait', visibility: { external_mcp: true } },
  { name: 'send_message', description: 'Deliver a correction or steering message to a worker.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, message: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/send', visibility: { external_mcp: true } },
  { name: 'followup_task', description: 'Give the same worker a new task as the next turn. Waits up to 120 seconds and returns status/result; call wait_agents only if still queued or running.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, task: { type: 'string' } }, required: ['handle', 'task'] }, method: 'POST', path: '/agents/followup', visibility: { external_mcp: true } },
  { name: 'interrupt_agent', description: 'Stop the current turn without disposing the worker.', inputSchema: { type: 'object', properties: { handle: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/cancel', visibility: { external_mcp: true } },
  { name: 'inspect_agent', description: 'Inspect a worker handle: status, task, capabilities.', inputSchema: { type: 'object', properties: { handle: { type: 'string' } }, required: ['handle'] }, method: 'GET', path: '/agents/{handle}', visibility: { external_mcp: true } },
  { name: 'agent_transcript', description: 'Read typed transcript events for a worker.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, after: { type: 'integer' } }, required: ['handle'] }, method: 'GET', path: '/agents/{handle}/transcript', visibility: { external_mcp: true } },
  { name: 'close_agent', description: 'Explicitly end a worker session.', inputSchema: { type: 'object', properties: { handle: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/close', visibility: { external_mcp: true } },
  { name: 'send_agent', description: 'Compatibility wrapper for followup_task.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, task: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/followup', visibility: { external_mcp: true } },
  { name: 'cancel_agent', description: 'Compatibility wrapper for interrupt_agent.', inputSchema: { type: 'object', properties: { handle: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/cancel', visibility: { external_mcp: true } },
  { name: 'answer_prompt', description: 'Answer a worker approval prompt. Requires handle or pane_id plus prompt_id and choice.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, pane_id: { type: 'string' }, project_path: { type: 'string' }, prompt_id: { type: 'string' }, choice: { type: 'string' }, allow_broad: { type: 'boolean' } }, required: ['prompt_id', 'choice'], anyOf: [{ required: ['handle'] }, { required: ['pane_id'] }] }, method: 'POST', path: '/agents/answer', visibility: { external_mcp: true } },
  { name: 'list_agents', description: 'List workers (managed runs and UI panes) this connection can adopt.', inputSchema: { type: 'object', properties: { project_path: { type: 'string' } } }, method: 'GET', path: '/agents', visibility: { external_mcp: true } },
  { name: 'list_workers', description: 'Same as list_agents: discover UI panes and managed runs, then adopt with run_agent(worker_id).', inputSchema: { type: 'object', properties: { project_path: { type: 'string' } } }, method: 'GET', path: '/agents', visibility: { external_mcp: true } },
  { name: 'take_over', description: 'Take control of a worker pane.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, pane_id: { type: 'string' }, grant: { type: 'boolean' } } }, method: 'POST', path: '/agents/take-over', visibility: { external_mcp: true } },
  { name: 'session_identity', description: 'Read this connection\'s coordinator identity and attach token.', inputSchema: { type: 'object', properties: {} }, method: 'GET', path: '/mcp/session', visibility: { external_mcp: true } },
  { name: 'attach_agents', description: 'Re-attach after reconnect using attach_token.', inputSchema: { type: 'object', properties: { attach_token: { type: 'string' } }, required: ['attach_token'] }, method: 'POST', path: '/agents/attach', visibility: { external_mcp: true } },
  { name: 'release_lease', description: 'Drop the worker control lease.', inputSchema: { type: 'object', properties: { handle: { type: 'string' } }, required: ['handle'] }, method: 'POST', path: '/agents/release-lease', visibility: { external_mcp: true } },
  { name: 'transfer_agent', description: 'Transfer worker ownership to a live connection.', inputSchema: { type: 'object', properties: { handle: { type: 'string' }, to_session_id: { type: 'string' } }, required: ['handle', 'to_session_id'] }, method: 'POST', path: '/agents/transfer', visibility: { external_mcp: true } },
];

function mergeLocalAgentCatalog(remote: RegistryTool[]): RegistryTool[] {
  const names = new Set(remote.map((tool) => tool.name));
  const missing = FALLBACK_AGENT_TOOLS.filter((tool) => !names.has(tool.name));
  if (missing.length > 0) {
    log(
      'catalog mismatch: HTTP registry is missing',
      missing.map((tool) => tool.name).join(', '),
      '; advertising local agent tools so hosts do not fall back to raw HTTP',
    );
  }
  return [...remote, ...missing];
}

async function listRegistryTools(clientRef: { current: BridgeClient }) {
  try {
    const tools = mergeLocalAgentCatalog(await readRegistryTools(clientRef));
    return tools
      .filter((tool) => tool.visibility?.external_mcp !== false)
      .map((tool) => ({
        name: tool.name,
        description: tool.description,
        inputSchema: tool.inputSchema,
        ...(tool.outputSchema ? { outputSchema: tool.outputSchema } : {}),
      }));
  } catch (err) {
    log(
      'tool registry unavailable via HTTP; using local agent catalog.',
      err instanceof Error ? err.message : err,
      'Rebuild npm run build:mcp if the Rust puppet-master-mcp binary is missing.',
    );
    return FALLBACK_AGENT_TOOLS.map((tool) => ({
      name: tool.name,
      description: tool.description,
      inputSchema: tool.inputSchema,
    }));
  }
}

async function readRegistryTools(clientRef: { current: BridgeClient }): Promise<RegistryTool[]> {
  const response = await callWithRefresh<RegistryTool[] | { tools?: RegistryTool[] }>(clientRef, 'GET', '/mcp/tools');
  const tools = Array.isArray(response) ? response : response?.tools;
  if (!Array.isArray(tools)) throw new Error('tool registry returned an invalid tools payload');
  return tools;
}

type StartMode = 'agent' | 'shell' | 'both';

/** Starting tool mode from `--mode <mode>` / `--mode=<mode>` (wins) or `PUPPET_MASTER_MODE`. */
function initialModeFrom(env: NodeJS.ProcessEnv, argv: string[]): StartMode | undefined {
  let raw = env.PUPPET_MASTER_MODE;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i] ?? '';
    if (arg === '--mode') raw = argv[i + 1];
    else if (arg.startsWith('--mode=')) raw = arg.slice('--mode='.length);
  }
  const value = raw?.trim().toLowerCase();
  if (!value) return undefined;
  if (value === 'agent' || value === 'shell' || value === 'both') return value;
  log('ignoring invalid starting mode', JSON.stringify(raw), '(expected agent, shell, or both)');
  return undefined;
}

async function main(): Promise<void> {
  log('starting');
  const sessionId = randomUUID();
  let client: BridgeClient;
  try {
    client = await makeClient(sessionId);
    log('connected to bridge', client.baseUrl);
    try {
      const instructions = await call<{ instructions?: string }>(client, 'GET', '/mcp/instructions');
      if (instructions.instructions) log('instructions:', instructions.instructions);
      else log('instructions:', MCP_INSTRUCTIONS);
    } catch {
      log('instructions:', MCP_INSTRUCTIONS);
    }
  } catch (err) {
    log('bridge unavailable:', err instanceof Error ? err.message : err);
    // We still start the server so the host gets a clean error rather than
    // a hung stdio. Each tool call will return the friendly error.
    client = { baseUrl: 'http://127.0.0.1:0', sessionId };
  }
  const clientRef = { current: client };

  // Hosts that read the tool list once (and ignore tools/list_changed) choose the catalog at launch.
  const initialMode = initialModeFrom(process.env, process.argv.slice(2));
  let initialModeApplied = initialMode === undefined;
  const applyInitialMode = async (): Promise<void> => {
    if (initialModeApplied || initialMode === undefined) return;
    try {
      try {
        await call<unknown>(clientRef.current, 'POST', '/mcp/mode', { mode: initialMode });
      } catch {
        clientRef.current = await makeClient(clientRef.current.sessionId);
        await call<unknown>(clientRef.current, 'POST', '/mcp/mode', { mode: initialMode });
      }
      initialModeApplied = true;
      log('starting in', initialMode, 'mode');
    } catch (err) {
      log('could not apply starting mode yet:', err instanceof Error ? err.message : err);
    }
  };
  await applyInitialMode();

  const server = new Server(
    { name: 'puppet-master', version: '0.1.3' },
    { capabilities: { tools: { listChanged: true } } },
  );

  log(
    MCP_INSTRUCTIONS,
  );
  log(
    'delegate_task only prepares a prompt. Use delegate_work to create an asynchronous operation and wait_for_operation for revision changes. get_operation is for inspection; cancel_operation requests cancellation. Never automatically retry mutations unless using the same idempotency_key.',
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => {
    await applyInitialMode();
    return { tools: await listRegistryTools(clientRef) };
  });

  server.setRequestHandler(CallToolRequestSchema, async (request, extra) => {
      const { name, arguments: args } = request.params;
    const t0 = Date.now();
    await applyInitialMode();
    log('tool call', name, JSON.stringify(args));
    try {
      let text = '';
      let catalogChanged = false;
      switch (name) {
        case 'list_panes': {
          const panes = PaneInfoSchema.array().parse(
            await callWithRefresh<unknown[]>(clientRef, 'GET', '/panes'),
          );
          text = formatPaneListForOrchestrator(panes);
          break;
        }
        case 'bridge_health': {
          const health = await callWithRefresh<unknown>(clientRef, 'GET', '/health');
          text = JSON.stringify(health, null, 2);
          break;
        }
        case 'list_agent_contexts': {
          const contexts = await callWithRefresh<unknown[]>(clientRef, 'GET', '/agent-contexts');
          text = JSON.stringify(contexts, null, 2);
          break;
        }
        case 'read_agent_context': {
          const a = (args ?? {}) as { agent_type?: string; pane_id?: string };
          if (a.pane_id) {
            const context = await callWithRefresh<unknown>(clientRef, 'GET', `/panes/${encodeURIComponent(a.pane_id)}/agent-context`);
            text = JSON.stringify(context, null, 2);
            break;
          }
          const agentType = AgentTypeSchema.parse(a.agent_type);
          const contexts = await callWithRefresh<Array<{ agent_type: string }>>(clientRef, 'GET', '/agent-contexts');
          const context = contexts.find((candidate) => candidate.agent_type === agentType);
          if (!context) throw new Error(`unknown agent_type: ${agentType}`);
          text = JSON.stringify(context, null, 2);
          break;
        }
        case 'inspect_agent_model': {
          const a = (args ?? {}) as { pane_id: string; lines?: number };
          const model = await callWithRefresh<unknown>(
            clientRef,
            'GET',
            `/panes/${encodeURIComponent(a.pane_id)}/model?lines=${a.lines ?? 200}`,
          );
          text = JSON.stringify(model, null, 2);
          break;
        }
        case 'switch_agent_model': {
          const a = args as { pane_id: string; model_id: string; model_provider?: string };
          assertWorkerPaneTarget(a.pane_id);
          const parsed = SwitchModelRequestSchema.parse({
            model_id: a.model_id,
            model_provider: a.model_provider,
          });
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/panes/${encodeURIComponent(a.pane_id)}/model`,
            parsed,
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'spawn_agent': {
          const parsed = SpawnPaneRequestSchema.parse(args);
          const forceNew = (args as { force_new?: boolean } | undefined)?.force_new === true;
          if (parsed.pane_id) {
            assertWorkerPaneTarget(parsed.pane_id);
          } else if (!forceNew) {
            const panes = PaneInfoSchema.array().parse(
              await callWithRefresh<unknown[]>(clientRef, 'GET', '/panes'),
            );
            const reusable = findReusableWorkerPane(panes, parsed.agent_type);
            if (reusable) {
              text = `reusing existing worker pane: ${reusable.id} (agent=${reusable.agent_type}, status=${reusable.status})`;
              break;
            }
          }
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/panes', parsed);
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'read_terminal_buffer': {
          const a = (args ?? {}) as { pane_id: string; lines?: number; view?: 'screen' | 'scrollback' };
          const lines = a.lines ?? 200;
          const view = a.view ? `&view=${encodeURIComponent(a.view)}` : '';
          const result = await callWithRefresh<{ content: string }>(
            clientRef,
            'GET',
            `/panes/${encodeURIComponent(a.pane_id)}/buffer?lines=${lines}${view}`,
          );
          text = result.content;
          break;
        }
        case 'write_terminal_input': {
          const parsed = WriteInputRequestSchema.parse({ ...(args as object), pane_id: undefined });
          const a = args as { pane_id: string; text: string; append_newline?: boolean };
          assertWorkerPaneTarget(a.pane_id);
          const result = await callWithRefresh<unknown>(clientRef, 'POST', `/panes/${encodeURIComponent(a.pane_id)}/input`, {
            text: a.text,
            append_newline: parsed.append_newline,
            via_opencode_api: parsed.via_opencode_api ?? true,
            ...(parsed.model_provider ? { model_provider: parsed.model_provider } : {}),
            ...(parsed.model_id ? { model_id: parsed.model_id } : {}),
          });
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'kill_pane_process': {
          const a = args as { pane_id: string };
          assertWorkerPaneTarget(a.pane_id);
          await callWithRefresh(clientRef, 'DELETE', `/panes/${encodeURIComponent(a.pane_id)}`);
          text = 'killed';
          break;
        }
        case 'create_task': {
          const a = args as { title: string; exclusive?: boolean };
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/tasks', {
            title: a.title,
            exclusive: a.exclusive ?? true,
          });
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'claim_task': {
          const a = args as { task_id: string; agent_id: string; lease_ms?: number };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/tasks/${encodeURIComponent(a.task_id)}/claim`,
            { agent_id: a.agent_id, lease_ms: a.lease_ms },
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'report_task_status': {
          const a = args as { task_id: string; status: string; agent_id?: string; reason?: string; project_path?: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/tasks/${encodeURIComponent(a.task_id)}/status`,
            { status: a.status, agent_id: a.agent_id, reason: a.reason, project_path: a.project_path },
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'complete_task': {
          const a = args as { task_id: string; agent_id: string; evidence?: string; project_path?: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/tasks/${encodeURIComponent(a.task_id)}/complete${typeof a.project_path === 'string' ? `?project_path=${encodeURIComponent(a.project_path)}` : ''}`,
            { agent_id: a.agent_id, evidence: a.evidence },
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'list_tasks': {
          const tasks = await callWithRefresh<unknown[]>(clientRef, 'GET', '/tasks');
          text = JSON.stringify(tasks, null, 2);
          break;
        }
        case 'acquire_resource_lock': {
          const a = args as {
            resource_type: string;
            name: string;
            owner_id: string;
            lease_ms?: number;
          };
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/locks', a);
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'release_resource_lock': {
          const a = args as { resource_type: string; name: string; owner_id: string };
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/locks/release', a);
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'build_context_pack': {
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/context-packs', args ?? {});
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'read_session_context': {
          const result = await callWithRefresh<unknown>(clientRef, 'GET', '/session/context');
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'update_session_context': {
          const result = await callWithRefresh<unknown>(clientRef, 'PATCH', '/session/context', args ?? {});
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'set_pane_role': {
          const a = args as { pane_id: string; role: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/panes/${encodeURIComponent(a.pane_id)}/role`,
            { role: a.role },
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'read_pane_digest': {
          const a = args as { pane_id: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'GET',
            `/panes/${encodeURIComponent(a.pane_id)}/digest`,
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'update_pane_digest': {
          const a = args as { pane_id: string; summary: string; source?: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/panes/${encodeURIComponent(a.pane_id)}/digest`,
            a,
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'delegate_task': {
          const result = await callWithRefresh<unknown>(clientRef, 'POST', '/delegate-task', args ?? {});
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'delegate_work':
        case 'get_operation':
        case 'wait_for_operation':
        case 'cancel_operation': {
          const a = (args ?? {}) as Record<string, unknown>;
          const id = typeof a.operation_id === 'string' ? encodeURIComponent(a.operation_id) : '';
          let method = 'GET';
          let path = `/operations/${id}`;
          let body: unknown;
          if (name === 'delegate_work') { method = 'POST'; path = '/operations/delegate'; body = a; }
          else if (name === 'wait_for_operation') { method = 'POST'; path += '/wait'; body = Object.fromEntries(Object.entries(a).filter(([key]) => key !== 'operation_id')); }
          else if (name === 'cancel_operation') { method = 'POST'; path += '/cancel'; body = {}; }
          const project = typeof a.project_path === 'string' ? `?project_path=${encodeURIComponent(a.project_path)}` : '';
          if (name !== 'delegate_work' && project) path += project;
          const timeout = name === 'wait_for_operation' ? waitTimeoutMs(a) : 30_000;
          if (name === 'wait_for_operation' && extra._meta?.progressToken !== undefined) {
            const requestedTimeout = typeof a.timeout_ms === 'number' ? Math.max(0, Math.min(a.timeout_ms, 300_000)) : 120_000;
            const result = await waitForOperationWithProgress(clientRef.current, path, body as Record<string, unknown>, requestedTimeout, extra.signal, async (snapshot) => {
              await extra.sendNotification({ method: 'notifications/progress', params: { progressToken: extra._meta!.progressToken!, progress: Number((snapshot as { revision?: number }).revision ?? 0), message: JSON.stringify(snapshot) } });
            });
            text = JSON.stringify(result, null, 2);
          } else {
            const result = await callWithTimeout<unknown>(clientRef.current, method, path, body, timeout, extra.signal);
            text = JSON.stringify(result, null, 2);
          }
          break;
        }
        case 'read_orchestrator_state': {
          const result = await callWithRefresh<unknown>(clientRef, 'GET', '/orchestrator/state');
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'update_orchestrator_state': {
          const result = await callWithRefresh<unknown>(clientRef, 'PATCH', '/orchestrator/state', args ?? {});
          text = JSON.stringify(result, null, 2);
          break;
        }
        default: {
          const result = await invokeRegistryTool(clientRef, name, (args ?? {}) as Record<string, unknown>);
          text = typeof result === 'string' ? result : JSON.stringify(result, null, 2);
          catalogChanged = name === 'set_mode';
          if (catalogChanged) initialModeApplied = true;
        }
      }
      if (catalogChanged) {
        await extra.sendNotification({ method: 'notifications/tools/list_changed' });
        text = withSetModeNote(text);
      }
      log('tool done', name, `${Date.now() - t0}ms`);
      const parsed = tryJson(text);
      const structuredContent = parsed === undefined ? undefined : isRecord(parsed) ? parsed : { data: parsed };
      return { content: [{ type: 'text' as const, text }], ...(structuredContent ? { structuredContent } : {}) };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log('tool error', name, msg);
      const detail = err instanceof ToolCallError ? err.detail : classifyToolError(err, msg, name);
      return { content: [{ type: 'text' as const, text: `error: ${detail.message}` }], isError: true, structuredContent: detail };
    }
  });

  const transport = new StdioServerTransport();
  await server.connect(transport);
  log('ready');
}

const SET_MODE_NOTE =
  'Sent notifications/tools/list_changed. Hosts that read tools/list only once at startup ignore it and keep the old tool list; if tool_count differs from what you can see, restart the MCP server with --mode <mode> or reconnect.';

function withSetModeNote(text: string): string {
  const parsed = tryJson(text);
  if (!isRecord(parsed)) return `${text}

Note: ${SET_MODE_NOTE}`;
  return JSON.stringify({ ...parsed, tools_list_changed_sent: true, note: SET_MODE_NOTE }, null, 2);
}

function tryJson(text: string): unknown | undefined {
  try { return JSON.parse(text); } catch { return undefined; }
}

function bridgeHttpToolError(status: number, body: string, route: string): ToolCallError {
  try {
    const parsed = JSON.parse(body) as BridgeToolError & { error?: unknown };
    if (typeof parsed.code === 'string' && typeof parsed.recoverable === 'boolean') {
      const message = typeof parsed.message === 'string' && parsed.message
        ? parsed.message
        : typeof parsed.error === 'string' && parsed.error
          ? parsed.error
          : `bridge request failed: ${route}`;
      return new ToolCallError({ ...parsed, message });
    }
  } catch { /* map legacy `{error}` bodies below */ }
  const mapping: Record<number, [string, boolean, number?]> = {
    400: ['INVALID_ARGUMENT', false], 401: ['AUTHORIZATION_DENIED', false], 403: ['AUTHORIZATION_DENIED', false],
    404: ['NOT_FOUND', false], 409: ['CONFLICT', true, 1000], 429: ['RATE_LIMITED', true, 1000],
  };
  const [code, recoverable, retry_after_ms] = mapping[status] ?? (status >= 500 ? ['BRIDGE_UNAVAILABLE', true] : ['BRIDGE_ERROR', true]);
  let message = body;
  try { const parsed = JSON.parse(body) as { error?: unknown }; if (typeof parsed.error === 'string') message = parsed.error; } catch { /* retain plain response */ }
  return new ToolCallError({ code, message: message || `bridge request failed: ${route}`, recoverable, ...(retry_after_ms === undefined ? {} : { retry_after_ms }), context: { http_status: status, route } });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function classifyToolError(error: unknown, message: string, tool: string): BridgeToolError {
  const invalid = error instanceof Error && (error.name === 'ZodError' || /missing required|unknown tool|unknown agent_type|refusing to target|must be/i.test(message));
  return { code: invalid ? 'INVALID_ARGUMENT' : 'bridge_error', message, recoverable: !invalid, context: { tool } };
}

main().catch((err) => {
  log('fatal', err);
  process.exit(1);
});
