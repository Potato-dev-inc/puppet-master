import {
  AgentTypeSchema,
  assertWorkerPaneTarget,
  findReusableWorkerPane,
  formatPaneListForOrchestrator,
  type AgentContextProfile,
  type AgentModelInspection,
  type PaneInfo,
} from '@puppet-master/shared';
import type { ToolDef } from './llm';
import type {
  BridgeClient,
  ContextPackRequest,
  DelegateTaskRequest,
  OrchestratorStateProjection,
  PaneRole,
} from './bridge';
import { tauri } from './tauri';
import { isTuiAgent, sleep, summarizeBuffer } from './ansi';
import { autoApprovePermissions, pressKey, typeAndSubmit } from './tui-autopilot';

function formatPaneList(panes: PaneInfo[]): string {
  return formatPaneListForOrchestrator(panes);
}

export { formatPaneList };

/** Prefer an already-open worker pane of the same agent (never the orchestrator pane). */
function findReusablePane(panes: PaneInfo[], agentType: string): PaneInfo | undefined {
  return findReusableWorkerPane(panes, agentType);
}

async function waitForPaneReady(
  executor: McpToolExecutor,
  paneId: string,
  maxMs = 6000,
): Promise<PaneInfo | null> {
  const deadline = Date.now() + maxMs;
  while (Date.now() < deadline) {
    const panes = await executor.listPanes();
    const pane = panes.find((p) => p.id === paneId);
    if (pane && (pane.status === 'waiting_input' || pane.status === 'idle')) {
      return pane;
    }
    await sleep(200);
  }
  return null;
}

export const PUPPET_MASTER_TOOLS: ToolDef[] = [];

/** Executes the 5 MCP tools — via Tauri in the desktop app, or HTTP bridge externally. */
export interface McpToolExecutor {
  listToolDefinitions?(): Promise<ToolDef[]>;
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
  listAgentContexts(): Promise<AgentContextProfile[]>;
  readAgentContext(args: { agent_type?: string; pane_id?: string }): Promise<unknown>;
  inspectAgentModel(paneId: string, lines?: number): Promise<AgentModelInspection>;
  writeInput(paneId: string, text: string, appendNewline?: boolean): Promise<void>;
  createTask(args: { title: string; exclusive?: boolean }): Promise<{ task_id: string }>;
  claimTask(taskId: string, args: { agent_id: string; lease_ms?: number }): Promise<unknown>;
  reportTaskStatus(taskId: string, args: { status: string }): Promise<unknown>;
  completeTask(taskId: string, args: { agent_id: string; evidence?: string }): Promise<unknown>;
  listTasks(): Promise<unknown[]>;
  acquireResourceLock(args: {
    resource_type: string;
    name: string;
    owner_id: string;
    lease_ms?: number;
  }): Promise<unknown>;
  releaseResourceLock(args: {
    resource_type: string;
    name: string;
    owner_id: string;
  }): Promise<unknown>;
  buildContextPack(args: ContextPackRequest): Promise<unknown>;
  readSessionContext(): Promise<unknown>;
  updateSessionContext(args: { current_goal?: string | null }): Promise<unknown>;
  setPaneRole(paneId: string, role: PaneRole): Promise<unknown>;
  readPaneDigest(paneId: string): Promise<unknown>;
  updatePaneDigest(args: { pane_id: string; summary: string; source?: string }): Promise<unknown>;
  delegateTask(args: DelegateTaskRequest): Promise<unknown>;
  readOrchestratorState(): Promise<unknown>;
  updateOrchestratorState(args: Partial<OrchestratorStateProjection>): Promise<unknown>;
}

function bridgeRequiredTool(name: string): never {
  throw new Error(`${name} requires the Rust HTTP bridge executor`);
}

/** Direct Rust PTY path — used by the built-in Puppet Master sidebar. */
export function makeTauriExecutor(): McpToolExecutor {
  return {
    listPanes: () => tauri.listPanes() as Promise<PaneInfo[]>,
    spawnPane: async (args) => {
      const pane_id = await tauri.spawnPane(args);
      return { pane_id };
    },
    killPane: (paneId) => tauri.killPane(paneId),
    readBuffer: (paneId, lines) => tauri.readBuffer(paneId, lines),
    listAgentContexts: () => tauri.listAgentContexts(),
    readAgentContext: (args) => tauri.readAgentContext(args),
    inspectAgentModel: (paneId, lines) => tauri.inspectAgentModel(paneId, lines),
    writeInput: (paneId, text, appendNewline) => tauri.writeInput(paneId, text, appendNewline),
    createTask: () => bridgeRequiredTool('create_task'),
    claimTask: () => bridgeRequiredTool('claim_task'),
    reportTaskStatus: () => bridgeRequiredTool('report_task_status'),
    completeTask: () => bridgeRequiredTool('complete_task'),
    listTasks: () => bridgeRequiredTool('list_tasks'),
    acquireResourceLock: () => bridgeRequiredTool('acquire_resource_lock'),
    releaseResourceLock: () => bridgeRequiredTool('release_resource_lock'),
    buildContextPack: () => bridgeRequiredTool('build_context_pack'),
    readSessionContext: () => bridgeRequiredTool('read_session_context'),
    updateSessionContext: () => bridgeRequiredTool('update_session_context'),
    setPaneRole: () => bridgeRequiredTool('set_pane_role'),
    readPaneDigest: () => bridgeRequiredTool('read_pane_digest'),
    updatePaneDigest: () => bridgeRequiredTool('update_pane_digest'),
    delegateTask: () => bridgeRequiredTool('delegate_task'),
    readOrchestratorState: () => bridgeRequiredTool('read_orchestrator_state'),
    updateOrchestratorState: () => bridgeRequiredTool('update_orchestrator_state'),
  };
}

/** HTTP bridge path — for external MCP clients and optional fallback. */
export function makeBridgeExecutor(bridge: BridgeClient): McpToolExecutor {
  return {
    listToolDefinitions: async () => {
      const tools = await bridge.listMcpTools();
      return tools
        .filter((tool) => tool.visibility.sidebar)
        .map((tool) => ({
          name: tool.name,
          description: tool.description,
          input_schema: tool.inputSchema,
        }));
    },
    listPanes: () => bridge.listPanes(),
    spawnPane: (args) => bridge.spawnPane(args),
    killPane: (id) => bridge.killPane(id),
    readBuffer: (id, lines) => bridge.readBuffer(id, lines),
    listAgentContexts: () => bridge.listAgentContexts(),
    readAgentContext: (args) => bridge.readAgentContext(args),
    inspectAgentModel: (paneId, lines) => bridge.inspectAgentModel(paneId, lines),
    writeInput: (id, text, appendNewline) => bridge.writeInput(id, text, appendNewline),
    createTask: (args) => bridge.createTask(args),
    claimTask: (taskId, args) => bridge.claimTask(taskId, args),
    reportTaskStatus: (taskId, args) => bridge.patchTaskStatus(taskId, args),
    completeTask: (taskId, args) => bridge.completeTask(taskId, args),
    listTasks: () => bridge.listTasks(),
    acquireResourceLock: (args) => bridge.acquireResourceLock(args),
    releaseResourceLock: (args) => bridge.releaseResourceLock(args),
    buildContextPack: (args) => bridge.buildContextPack(args),
    readSessionContext: () => bridge.readSessionContext(),
    updateSessionContext: (args) => bridge.updateSessionContext(args),
    setPaneRole: (paneId, role) => bridge.setPaneRole(paneId, role),
    readPaneDigest: (paneId) => bridge.readPaneDigest(paneId),
    updatePaneDigest: (args) => bridge.updatePaneDigest(args),
    delegateTask: (args) => bridge.delegateTask(args),
    readOrchestratorState: () => bridge.readOrchestratorState(),
    updateOrchestratorState: (args) => bridge.updateOrchestratorState(args),
  };
}

export async function loadPuppetMasterTools(executor: McpToolExecutor): Promise<ToolDef[]> {
  if (!executor.listToolDefinitions) return PUPPET_MASTER_TOOLS;
  try {
    const tools = await executor.listToolDefinitions();
    return tools.length > 0 ? tools : PUPPET_MASTER_TOOLS;
  } catch {
    return PUPPET_MASTER_TOOLS;
  }
}

export async function executeMcpTool(
  executor: McpToolExecutor,
  name: string,
  args: Record<string, unknown>,
  onLog: (entry: {
    source: 'builtin';
    tool: string;
    args: unknown;
    result_preview?: string;
    error?: string;
    duration_ms: number;
  }) => void,
): Promise<string> {
  const t0 = performance.now();
  try {
    let result = '';
    switch (name) {
      case 'list_panes': {
        const panes = await executor.listPanes();
        result = formatPaneList(panes);
        break;
      }
      case 'list_agent_contexts': {
        result = JSON.stringify(await executor.listAgentContexts(), null, 2);
        break;
      }
      case 'read_agent_context': {
        const a = args as { agent_type?: string; pane_id?: string };
        if (a.pane_id) {
          result = JSON.stringify(await executor.readAgentContext({ pane_id: a.pane_id }), null, 2);
          break;
        }
        const agentType = AgentTypeSchema.parse(a.agent_type);
        result = JSON.stringify(await executor.readAgentContext({ agent_type: agentType }), null, 2);
        break;
      }
      case 'inspect_agent_model': {
        const a = args as { pane_id: string; lines?: number };
        result = JSON.stringify(await executor.inspectAgentModel(a.pane_id, a.lines), null, 2);
        break;
      }
      case 'spawn_agent': {
        const spawnArgs = args as Parameters<McpToolExecutor['spawnPane']>[0] & {
          force_new?: boolean;
        };
        const forceNew = spawnArgs.force_new === true;

        if (spawnArgs.pane_id) {
          assertWorkerPaneTarget(spawnArgs.pane_id);
        }

        if (!forceNew && !spawnArgs.pane_id) {
          const existingPanes = await executor.listPanes();
          const reusable = findReusablePane(existingPanes, spawnArgs.agent_type);
          if (reusable) {
            if (isTuiAgent(spawnArgs.agent_type)) {
              const ready = await waitForPaneReady(executor, reusable.id);
              const approved = await autoApprovePermissions(executor, reusable.id, 2000);
              const approveNote = approved ? `; ${approved}` : '';
              result = ready
                ? `reusing existing pane: ${reusable.id} (agent=${reusable.agent_type}, status=${ready.status}${approveNote})`
                : `reusing existing pane: ${reusable.id} (agent=${reusable.agent_type}, still booting${approveNote})`;
            } else {
              result = `reusing existing pane: ${reusable.id} (agent=${reusable.agent_type})`;
            }
            break;
          }
        }

        const r = await executor.spawnPane(spawnArgs);
        if (isTuiAgent(spawnArgs.agent_type)) {
          const ready = await waitForPaneReady(executor, r.pane_id);
          const approved = await autoApprovePermissions(executor, r.pane_id, 3000);
          const approveNote = approved ? `; ${approved}` : '';
          result = ready
            ? `spawned pane: ${r.pane_id} (status=${ready.status}, ready for input${approveNote})`
            : `spawned pane: ${r.pane_id} (still booting${approveNote})`;
        } else {
          result = `spawned pane: ${r.pane_id}`;
        }
        break;
      }
      case 'read_terminal_buffer': {
        const a = args as { pane_id: string; lines?: number };
        const raw = await executor.readBuffer(a.pane_id, a.lines ?? 40);
        result = summarizeBuffer(raw);
        if (!result) result = '(empty buffer — agent may still be loading)';
        break;
      }
      case 'write_terminal_input': {
        const a = args as { pane_id: string; text: string; append_newline?: boolean };
        assertWorkerPaneTarget(a.pane_id);
        const append = a.append_newline !== false;
        if (append) {
          await typeAndSubmit(executor, a.pane_id, a.text);
        } else {
          const text = a.text.replace(/[\r\n]+$/, '');
          await executor.writeInput(a.pane_id, text, false);
        }
        const approved = await autoApprovePermissions(executor, a.pane_id, append ? 20000 : 5000);
        result = append
          ? `typed + Enter${approved ? `; ${approved}` : ''}`
          : `typed ${a.text.length} chars${approved ? `; ${approved}` : ''}`;
        break;
      }
      case 'press_key': {
        const a = args as { pane_id: string; key: string };
        assertWorkerPaneTarget(a.pane_id);
        const r = await pressKey(executor, a.pane_id, a.key);
        if (!r.ok) throw new Error(r.error);
        result = `pressed ${r.key} (${r.bytes} byte${r.bytes === 1 ? '' : 's'})`;
        break;
      }
      case 'kill_pane_process': {
        const a = args as { pane_id: string };
        assertWorkerPaneTarget(a.pane_id);
        await executor.killPane(a.pane_id);
        result = 'killed';
        break;
      }
      case 'create_task': {
        const a = args as { title: string; exclusive?: boolean };
        result = JSON.stringify(await executor.createTask({
          title: a.title,
          exclusive: a.exclusive ?? true,
        }), null, 2);
        break;
      }
      case 'claim_task': {
        const a = args as { task_id: string; agent_id: string; lease_ms?: number };
        result = JSON.stringify(await executor.claimTask(a.task_id, {
          agent_id: a.agent_id,
          lease_ms: a.lease_ms,
        }), null, 2);
        break;
      }
      case 'report_task_status': {
        const a = args as { task_id: string; status: string };
        result = JSON.stringify(await executor.reportTaskStatus(a.task_id, {
          status: a.status,
        }), null, 2);
        break;
      }
      case 'complete_task': {
        const a = args as { task_id: string; agent_id: string; evidence?: string };
        result = JSON.stringify(await executor.completeTask(a.task_id, {
          agent_id: a.agent_id,
          evidence: a.evidence,
        }), null, 2);
        break;
      }
      case 'list_tasks': {
        result = JSON.stringify(await executor.listTasks(), null, 2);
        break;
      }
      case 'acquire_resource_lock': {
        const a = args as { resource_type: string; name: string; owner_id: string; lease_ms?: number };
        result = JSON.stringify(await executor.acquireResourceLock(a), null, 2);
        break;
      }
      case 'release_resource_lock': {
        const a = args as { resource_type: string; name: string; owner_id: string };
        result = JSON.stringify(await executor.releaseResourceLock(a), null, 2);
        break;
      }
      case 'build_context_pack': {
        result = JSON.stringify(await executor.buildContextPack(args as ContextPackRequest), null, 2);
        break;
      }
      case 'read_session_context': {
        result = JSON.stringify(await executor.readSessionContext(), null, 2);
        break;
      }
      case 'update_session_context': {
        result = JSON.stringify(
          await executor.updateSessionContext(args as { current_goal?: string | null }),
          null,
          2,
        );
        break;
      }
      case 'set_pane_role': {
        const a = args as { pane_id: string; role: PaneRole };
        result = JSON.stringify(await executor.setPaneRole(a.pane_id, a.role), null, 2);
        break;
      }
      case 'read_pane_digest': {
        const a = args as { pane_id: string };
        result = JSON.stringify(await executor.readPaneDigest(a.pane_id), null, 2);
        break;
      }
      case 'update_pane_digest': {
        result = JSON.stringify(
          await executor.updatePaneDigest(args as { pane_id: string; summary: string; source?: string }),
          null,
          2,
        );
        break;
      }
      case 'delegate_task': {
        result = JSON.stringify(await executor.delegateTask(args as unknown as DelegateTaskRequest), null, 2);
        break;
      }
      case 'read_orchestrator_state': {
        result = JSON.stringify(await executor.readOrchestratorState(), null, 2);
        break;
      }
      case 'update_orchestrator_state': {
        result = JSON.stringify(
          await executor.updateOrchestratorState(args as Partial<OrchestratorStateProjection>),
          null,
          2,
        );
        break;
      }
      default:
        throw new Error(`unknown tool: ${name}`);
    }
    onLog({
      source: 'builtin',
      tool: name,
      args,
      result_preview: result.slice(0, 200),
      duration_ms: Math.round(performance.now() - t0),
    });
    return result;
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    onLog({
      source: 'builtin',
      tool: name,
      args,
      error: msg,
      duration_ms: Math.round(performance.now() - t0),
    });
    throw err;
  }
}
