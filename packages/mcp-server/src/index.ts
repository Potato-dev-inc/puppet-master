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

const TOOLS = [] as const;

interface BridgeClient {
  baseUrl: string;
}

interface RegistryTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  visibility?: { external_mcp?: boolean };
  method?: string;
  path?: string;
}

function waitTimeoutMs(args: Record<string, unknown>): number {
  const timeout = typeof args.timeout_ms === 'number' ? args.timeout_ms : 120_000;
  return Math.min(timeout + 15_000, 310_000);
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
): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(url, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`bridge ${method} ${path} -> ${res.status}: ${text}`);
    }
    return (await res.json()) as T;
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      throw new Error(`bridge ${method} ${path} timed out after ${timeoutMs}ms`);
    }
    throw err;
  } finally {
    clearTimeout(timer);
  }
}

async function invokeRegistryTool(
  clientRef: { current: BridgeClient },
  name: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  const tools = await callWithRefresh<RegistryTool[]>(clientRef, 'GET', '/mcp/tools');
  const def = tools.find((tool) => tool.name === name);
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
    } else if (path.includes('{')) {
      path = resolveRegistryPath(path, args);
      path = appendQuery(path, args, ['lines']);
    }
  } else {
    path = path.includes('{') ? resolveRegistryPath(path, args) : path;
  }

  const timeoutMs = path.includes('/panes/wait') ? waitTimeoutMs(args) : 30_000;
  return callWithTimeout(clientRef.current, def.method, path, def.method === 'GET' ? undefined : args, timeoutMs);
}

async function makeClient(): Promise<BridgeClient> {
  const { host, port } = await readBridgePort();
  return { baseUrl: `http://${host}:${port}` };
}

async function call<T>(client: BridgeClient, method: string, path: string, body?: unknown): Promise<T> {
  const url = `${client.baseUrl}${path}`;
  let res: Response;
  try {
    res = await fetch(url, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch (err) {
    throw new Error(`bridge ${method} ${path} -> fetch failed at ${client.baseUrl}: ${err instanceof Error ? err.message : String(err)}`);
  }
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`bridge ${method} ${path} -> ${res.status}: ${text}`);
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
    const message = err instanceof Error ? err.message : String(err);
    if (!message.includes('fetch failed')) throw err;
    const next = await makeClient();
    clientRef.current = next;
    log('refreshed bridge client', next.baseUrl);
    return await call<T>(clientRef.current, method, path, body);
  }
}

async function listRegistryTools(clientRef: { current: BridgeClient }) {
  try {
    const tools = await callWithRefresh<RegistryTool[]>(clientRef, 'GET', '/mcp/tools');
    return tools
      .filter((tool) => tool.visibility?.external_mcp !== false)
      .map((tool) => ({
        name: tool.name,
        description: tool.description,
        inputSchema: tool.inputSchema,
      }));
  } catch (err) {
    log('tool registry unavailable:', err instanceof Error ? err.message : err);
    return [];
  }
}

async function main(): Promise<void> {
  log('starting');
  let client: BridgeClient;
  try {
    client = await makeClient();
    log('connected to bridge', client.baseUrl);
  } catch (err) {
    log('bridge unavailable:', err instanceof Error ? err.message : err);
    // We still start the server so the host gets a clean error rather than
    // a hung stdio. Each tool call will return the friendly error.
    client = { baseUrl: 'http://127.0.0.1:0' };
  }
  const clientRef = { current: client };

  const server = new Server(
    { name: 'puppet-master', version: '0.1.3' },
    { capabilities: { tools: {} } },
  );

  log(
    'wait-not-poll: after mutating tools call suggested_wait (wait_for_panes / wait_for_model). Do not loop list_panes or read_terminal_buffer for status.',
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: await listRegistryTools(clientRef),
  }));

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    const t0 = Date.now();
    log('tool call', name, JSON.stringify(args));
    try {
      let text = '';
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
          if (parsed.pane_id) {
            assertWorkerPaneTarget(parsed.pane_id);
          } else {
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
          const a = (args ?? {}) as { pane_id: string; lines?: number };
          const lines = a.lines ?? 200;
          const result = await callWithRefresh<{ content: string }>(
            clientRef,
            'GET',
            `/panes/${encodeURIComponent(a.pane_id)}/buffer?lines=${lines}`,
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
          const a = args as { task_id: string; status: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/tasks/${encodeURIComponent(a.task_id)}/status`,
            { status: a.status },
          );
          text = JSON.stringify(result, null, 2);
          break;
        }
        case 'complete_task': {
          const a = args as { task_id: string; agent_id: string; evidence?: string };
          const result = await callWithRefresh<unknown>(
            clientRef,
            'POST',
            `/tasks/${encodeURIComponent(a.task_id)}/complete`,
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
        }
      }
      log('tool done', name, `${Date.now() - t0}ms`);
      return { content: [{ type: 'text' as const, text }] };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log('tool error', name, msg);
      return { content: [{ type: 'text' as const, text: `error: ${msg}` }], isError: true };
    }
  });

  const transport = new StdioServerTransport();
  await server.connect(transport);
  log('ready');
}

main().catch((err) => {
  log('fatal', err);
  process.exit(1);
});
