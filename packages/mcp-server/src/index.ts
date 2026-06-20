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
  WriteInputRequestSchema,
} from '@puppet-master/shared';
import { readBridgePort } from '@puppet-master/shared/bridge-port';

const log = (...args: unknown[]) => {
  process.stderr.write(`[puppet-master-mcp] ${args.map(String).join(' ')}\n`);
};

const TOOLS = [
  {
    name: 'list_panes',
    description: 'List all live PTY panes. Panes with id puppet-master-orchestrator-* are the dedicated orchestrator — never write_terminal_input or kill them. Delegate only to worker panes.',
    inputSchema: { type: 'object' as const, properties: {}, required: [] },
  },
  {
    name: 'bridge_health',
    description: 'Check whether the Puppet Master HTTP bridge is reachable and return its version metadata. Orchestrators should call this first.',
    inputSchema: { type: 'object' as const, properties: {}, required: [] },
  },
  {
    name: 'list_agent_contexts',
    description: 'List the static context profiles for supported agents, including strengths, smartness score, and planned sidebar orchestration actions.',
    inputSchema: { type: 'object' as const, properties: {}, required: [] },
  },
  {
    name: 'read_agent_context',
    description: 'Read context for an agent type or a live pane. Orchestrators should call this before delegating to a pane. If pane_id is provided, includes pane metadata, model inspection, and a recent buffer preview.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        agent_type: { type: 'string', enum: ['claude', 'codex', 'opencode', 'powershell', 'bash', 'cursor'] },
        pane_id: { type: 'string' },
      },
      required: [],
    },
  },
  {
    name: 'inspect_agent_model',
    description: 'Inspect a live terminal pane and report the best-known model signal plus an advisory smartness score. Use this when choosing which running agent should receive a task.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        pane_id: { type: 'string' },
        lines: { type: 'number', description: 'Recent buffer lines to scan for model hints (default 200)' },
      },
      required: ['pane_id'],
    },
  },
  {
    name: 'spawn_agent',
    description: 'Spawn a worker PTY pane. Reuses existing worker panes of the same agent_type; never reuses puppet-master-orchestrator-* panes. Call list_panes first.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        agent_type: { type: 'string', enum: ['claude', 'codex', 'opencode', 'powershell', 'bash', 'cursor'] },
        cwd: { type: 'string', description: 'Working directory; defaults to current project root' },
        cols: { type: 'number', description: 'Terminal columns (default 120)' },
        rows: { type: 'number', description: 'Terminal rows (default 30)' },
        pane_id: { type: 'string', description: 'Optional caller-supplied stable id' },
      },
      required: ['agent_type'],
    },
  },
  {
    name: 'read_terminal_buffer',
    description: 'Read the recent scrollback of a pane as text (last N lines).',
    inputSchema: {
      type: 'object' as const,
      properties: {
        pane_id: { type: 'string' },
        lines: { type: 'number', description: 'How many trailing lines to return (default 200)' },
      },
      required: ['pane_id'],
    },
  },
  {
    name: 'write_terminal_input',
    description: 'Send keystrokes to a worker pane. Cannot target puppet-master-orchestrator-* panes. Use append_newline=true when delegating a prompt.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        pane_id: { type: 'string' },
        text: { type: 'string' },
        append_newline: { type: 'boolean', default: true },
      },
      required: ['pane_id', 'text'],
    },
  },
  {
    name: 'kill_pane_process',
    description: 'Terminate a worker pane. Cannot kill puppet-master-orchestrator-* panes.',
    inputSchema: {
      type: 'object' as const,
      properties: { pane_id: { type: 'string' } },
      required: ['pane_id'],
    },
  },
  {
    name: 'create_task',
    description: 'Create a coordination task in the Rust task board.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        title: { type: 'string' },
        exclusive: { type: 'boolean', default: true },
      },
      required: ['title'],
    },
  },
  {
    name: 'claim_task',
    description: 'Claim an exclusive task lease for an agent.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        task_id: { type: 'string' },
        agent_id: { type: 'string' },
        lease_ms: { type: 'number' },
      },
      required: ['task_id', 'agent_id'],
    },
  },
  {
    name: 'report_task_status',
    description: 'Update task status in the Rust task board.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        task_id: { type: 'string' },
        status: { type: 'string' },
      },
      required: ['task_id', 'status'],
    },
  },
  {
    name: 'complete_task',
    description: 'Complete a task with evidence.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        task_id: { type: 'string' },
        agent_id: { type: 'string' },
        evidence: { type: 'string' },
      },
      required: ['task_id', 'agent_id'],
    },
  },
  {
    name: 'list_tasks',
    description: 'List rebuildable task board state from the Rust event log.',
    inputSchema: { type: 'object' as const, properties: {}, required: [] },
  },
  {
    name: 'acquire_resource_lock',
    description: 'Acquire an exclusive resource lock.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        resource_type: { type: 'string', enum: ['file', 'directory', 'command', 'port', 'git branch', 'pane ownership'] },
        name: { type: 'string' },
        owner_id: { type: 'string' },
        lease_ms: { type: 'number' },
      },
      required: ['resource_type', 'name', 'owner_id'],
    },
  },
  {
    name: 'release_resource_lock',
    description: 'Release a resource lock owned by an agent or pane.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        resource_type: { type: 'string' },
        name: { type: 'string' },
        owner_id: { type: 'string' },
      },
      required: ['resource_type', 'name', 'owner_id'],
    },
  },
  {
    name: 'build_context_pack',
    description: 'Build a compact Rust-generated context pack for an assigned task.',
    inputSchema: {
      type: 'object' as const,
      properties: {
        task_id: { type: 'string' },
        agent_id: { type: 'string' },
        user_constraints: { type: 'array', items: { type: 'string' } },
        manager_instructions: { type: 'string' },
        raw_scrollback: { type: 'string' },
      },
      required: [],
    },
  },
];

interface BridgeClient {
  baseUrl: string;
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
    { name: 'puppet-master', version: '0.1.2' },
    { capabilities: { tools: {} } },
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

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
          const result = await callWithRefresh<{ pane_id: string }>(clientRef, 'POST', '/panes', parsed);
          text = `spawned pane: ${result.pane_id}`;
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
          await callWithRefresh(clientRef, 'POST', `/panes/${encodeURIComponent(a.pane_id)}/input`, {
            text: a.text,
            append_newline: parsed.append_newline,
          });
          text = 'ok';
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
        default:
          throw new Error(`unknown tool: ${name}`);
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
