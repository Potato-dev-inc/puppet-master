/** Generic dispatch of registry-defined tools to their bridge HTTP route. */
import { callWithTimeout } from './bridge-client.js';
import { FALLBACK_AGENT_TOOLS, readRegistryTools } from './catalog.js';
import { ensureIdempotencyKey, toolTimeoutMs, waitTimeoutError } from './timeouts.js';
import type { ClientRef } from './types.js';

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

export async function invokeRegistryTool(
  clientRef: ClientRef,
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
