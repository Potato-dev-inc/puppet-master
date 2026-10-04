/** Routes a tools/call request to the bridge: bespoke handlers for core tools, the registry otherwise. */
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
import type { RequestHandlerExtra } from '@modelcontextprotocol/sdk/shared/protocol.js';
import type { ServerNotification, ServerRequest } from '@modelcontextprotocol/sdk/types.js';
import { callWithRefresh, callWithTimeout } from './bridge-client.js';
import { invokeRegistryTool } from './registry-call.js';
import { waitForOperationWithProgress, waitTimeoutMs } from './timeouts.js';
import type { ClientRef } from './types.js';

export interface RoutedResult {
  text: string;
  /** True when the call changed the advertised tool list (set_mode). */
  catalogChanged: boolean;
}

export type ToolCallExtra = RequestHandlerExtra<ServerRequest, ServerNotification>;

export async function routeToolCall(
  clientRef: ClientRef,
  name: string,
  args: Record<string, unknown> | undefined,
  extra: ToolCallExtra,
): Promise<RoutedResult> {
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
    }
  }
  return { text, catalogChanged };
}
