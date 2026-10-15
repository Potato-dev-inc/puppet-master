#!/usr/bin/env node
/**
 * Puppet Master MCP server (Node fallback; the Rust binary is the primary server).
 *
 * Speaks stdio JSON-RPC using the `@modelcontextprotocol/sdk`. Each tool
 * call proxies to the local HTTP bridge (which fronts the Rust PTY manager
 * inside the running Tauri app).
 *
 * This file is the server entry: startup mode, handler registration, error
 * shaping. Module map: bridge-client (HTTP), catalog (tool list), tool-router
 * (tools/call dispatch), registry-call (registry tool routes), timeouts, errors.
 *
 * CRITICAL: every log line must go to stderr — never stdout — otherwise
 * we corrupt the JSON-RPC stream.
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { randomUUID } from 'node:crypto';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { call, makeClient } from './bridge-client.js';
import { listRegistryTools } from './catalog.js';
import { classifyToolError, ToolCallError } from './errors.js';
import { routeToolCall } from './tool-router.js';
import type { BridgeClient, ClientRef } from './types.js';
import { isRecord, log, tryJson } from './util.js';

const MCP_INSTRUCTIONS =
  'Puppet Master MCP (agent mode). Sequence: list_agents → run_agent(worker_id or pane_id, task) → handle; send_message(handle) steers the current turn; followup_task(handle) continues the conversation; wait_agents(handles) collects answers. Run watch_command in the background after run/followup/delegate or WAIT_TIMEOUT. Reuse idempotency_key after WAIT_TIMEOUT. Do not close the user pane as cleanup.';

const SET_MODE_NOTE =
  'Sent notifications/tools/list_changed. Hosts that read tools/list only once at startup ignore it and keep the old tool list; if tool_count differs from what you can see, restart the MCP server with --mode <mode> or reconnect.';

function withSetModeNote(text: string): string {
  const parsed = tryJson(text);
  if (!isRecord(parsed)) return `${text}

Note: ${SET_MODE_NOTE}`;
  return JSON.stringify({ ...parsed, tools_list_changed_sent: true, note: SET_MODE_NOTE }, null, 2);
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
  const clientRef: ClientRef = { current: client };

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
      const routed = await routeToolCall(clientRef, name, args, extra);
      let text = routed.text;
      if (routed.catalogChanged) {
        initialModeApplied = true;
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

main().catch((err) => {
  log('fatal', err);
  process.exit(1);
});
