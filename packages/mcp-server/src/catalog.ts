/** Tool catalog: the HTTP registry plus a built-in fallback of the agent tools. */
import { callWithRefresh } from './bridge-client.js';
import type { ClientRef, RegistryTool } from './types.js';
import { log } from './util.js';

/** Mirror of Rust `tool_registry::schema::agent_type_schema()`: every agent_type/backend input uses it. */
export const AGENT_TYPE_SCHEMA = {
  type: 'string',
  pattern: '^[a-z][a-z0-9_]{1,31}$',
  description: 'Agent id, e.g. claude, codex, cursor_agent, opencode_native. The registered ids and what each supports are listed by GET /agents/types; an unregistered id fails with UNKNOWN_AGENT_TYPE (context.valid lists the accepted ids).',
} as const;

export const FALLBACK_AGENT_TOOLS: RegistryTool[] = [
  { name: 'set_mode', description: 'Choose agent, shell, or both for this MCP connection.', inputSchema: { type: 'object', properties: { mode: { type: 'string', enum: ['agent', 'shell', 'both'] } }, required: ['mode'] }, method: 'POST', path: '/mcp/mode', visibility: { external_mcp: true } },
  { name: 'quick_ask', description: 'Ask a tiny one-shot question; runs a fresh headless read-only worker and returns {answer, receipt}. Still running after wait_ms: returns {status, handle}; call wait_agents.', inputSchema: { type: 'object', properties: { task: { type: 'string' }, project_path: { type: 'string' }, backend: AGENT_TYPE_SCHEMA, model: { type: 'string' }, wait_ms: { type: 'integer' } }, required: ['task'] }, method: 'POST', path: '/agents/quick', visibility: { external_mcp: true } },
  { name: 'run_agent', description: 'Start or adopt a worker with a task and return a stable handle. Pass worker_id from list_workers to adopt a UI pane; the pane directory is used automatically.', inputSchema: { type: 'object', properties: { task: { type: 'string' }, name: { type: 'string' }, handle: { type: 'string' }, worker_id: { type: 'string' }, project_path: { type: 'string' }, context_mode: { type: 'string' }, model: { type: 'string' }, agent_type: AGENT_TYPE_SCHEMA, background: { type: 'boolean' }, wait_ms: { type: 'integer' } }, required: ['task'] }, method: 'POST', path: '/agents/run', visibility: { external_mcp: true } },
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

export async function listRegistryTools(clientRef: ClientRef) {
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

export async function readRegistryTools(clientRef: ClientRef): Promise<RegistryTool[]> {
  const response = await callWithRefresh<RegistryTool[] | { tools?: RegistryTool[] }>(clientRef, 'GET', '/mcp/tools');
  const tools = Array.isArray(response) ? response : response?.tools;
  if (!Array.isArray(tools)) throw new Error('tool registry returned an invalid tools payload');
  return tools;
}
