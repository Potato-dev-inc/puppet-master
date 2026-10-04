// Tool executors for the built-in orchestrator: the McpToolExecutor contract plus its two backings,
// direct Tauri/PTY calls (desktop sidebar) and the HTTP bridge (external hosts / remote UI).
import type { AgentContextProfile, AgentModelInspection, PaneInfo } from '@puppet-master/shared';
import type {
  BridgeClient,
  ContextPackRequest,
  DelegateTaskRequest,
  OpenCodeWorkerStatus,
  OrchestratorStateProjection,
  PaneRole,
  WaitForPanesResult,
} from './bridge';
import type { ToolDef } from './llm';
import { tauri } from './tauri';

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
  switchAgentModel(
    paneId: string,
    args: { model_id: string; model_provider?: string },
  ): Promise<{ ok: boolean; provider_id: string; model_id: string; tui_synced?: boolean }>;
  writeInput(
    paneId: string,
    text: string,
    appendNewline?: boolean,
    options?: { viaOpencodeApi?: boolean; modelProvider?: string; modelId?: string },
  ): Promise<void>;
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
  readProjectIrStatus(): Promise<unknown>;
  readLibrarianPrompt(): Promise<unknown>;
  readSessionContext(): Promise<unknown>;
  updateSessionContext(args: { current_goal?: string | null }): Promise<unknown>;
  setPaneRole(paneId: string, role: PaneRole): Promise<unknown>;
  readPaneDigest(paneId: string): Promise<unknown>;
  updatePaneDigest(args: { pane_id: string; summary: string; source?: string }): Promise<unknown>;
  delegateTask(args: DelegateTaskRequest): Promise<unknown>;
  readOrchestratorState(): Promise<unknown>;
  updateOrchestratorState(args: Partial<OrchestratorStateProjection>): Promise<unknown>;
  waitForPanes?(args: {
    pane_ids: string[];
    until?: string[];
    timeout_ms?: number;
    task_id?: string;
    output_regex?: string;
    match?: { provider_id?: string; model_id?: string };
  }): Promise<WaitForPanesResult>;
  waitForModel?(args: {
    pane_id: string;
    provider_id?: string;
    model_id?: string;
    timeout_ms?: number;
  }): Promise<WaitForPanesResult>;
  waitForTask?(args: {
    pane_id: string;
    task_id: string;
    until?: string[];
    timeout_ms?: number;
  }): Promise<WaitForPanesResult>;
  readRecentEvents?(args?: {
    limit?: number;
    pane_id?: string;
    types?: string[];
    since_id?: string;
  }): Promise<unknown[]>;
  readOpencodeWorkerStatus?(paneId: string): Promise<OpenCodeWorkerStatus>;
  readOpencodeMessages?(
    paneId: string,
    args?: { limit?: number; role?: 'all' | 'user' | 'assistant' },
  ): Promise<unknown>;
  replyOpencodePermission?(paneId: string, requestId: string, reply: string): Promise<void>;
  replyOpencodeQuestion?(
    paneId: string,
    args: { answer: string; request_id?: string },
  ): Promise<unknown>;
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
    switchAgentModel: (paneId, args) => tauri.switchAgentModel(paneId, args),
    writeInput: (paneId, text, appendNewline, options) =>
      tauri.writeInput(paneId, text, appendNewline, options),
    createTask: () => bridgeRequiredTool('create_task'),
    claimTask: () => bridgeRequiredTool('claim_task'),
    reportTaskStatus: () => bridgeRequiredTool('report_task_status'),
    completeTask: () => bridgeRequiredTool('complete_task'),
    listTasks: () => bridgeRequiredTool('list_tasks'),
    acquireResourceLock: () => bridgeRequiredTool('acquire_resource_lock'),
    releaseResourceLock: () => bridgeRequiredTool('release_resource_lock'),
    buildContextPack: () => bridgeRequiredTool('build_context_pack'),
    readProjectIrStatus: () => bridgeRequiredTool('read_project_ir_status'),
    readLibrarianPrompt: () => bridgeRequiredTool('read_librarian_prompt'),
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
    switchAgentModel: (paneId, args) => bridge.switchAgentModel(paneId, args),
    writeInput: (id, text, appendNewline, options) =>
      bridge.writeInput(id, text, appendNewline, options),
    createTask: (args) => bridge.createTask(args),
    claimTask: (taskId, args) => bridge.claimTask(taskId, args),
    reportTaskStatus: (taskId, args) => bridge.patchTaskStatus(taskId, args),
    completeTask: (taskId, args) => bridge.completeTask(taskId, args),
    listTasks: () => bridge.listTasks(),
    acquireResourceLock: (args) => bridge.acquireResourceLock(args),
    releaseResourceLock: (args) => bridge.releaseResourceLock(args),
    buildContextPack: (args) => bridge.buildContextPack(args),
    readProjectIrStatus: () => bridge.readProjectIrStatus(),
    readLibrarianPrompt: () => bridge.readLibrarianPrompt(),
    readSessionContext: () => bridge.readSessionContext(),
    updateSessionContext: (args) => bridge.updateSessionContext(args),
    setPaneRole: (paneId, role) => bridge.setPaneRole(paneId, role),
    readPaneDigest: (paneId) => bridge.readPaneDigest(paneId),
    updatePaneDigest: (args) => bridge.updatePaneDigest(args),
    delegateTask: (args) => bridge.delegateTask(args),
    readOrchestratorState: () => bridge.readOrchestratorState(),
    updateOrchestratorState: (args) => bridge.updateOrchestratorState(args),
    waitForPanes: (args) => bridge.waitForPanes(args),
    waitForModel: (args) => bridge.waitForModel(args),
    waitForTask: (args) => bridge.waitForTask(args),
    readRecentEvents: (args) => bridge.readRecentEvents(args),
    readOpencodeWorkerStatus: (paneId) => bridge.readOpencodeWorkerStatus(paneId),
    readOpencodeMessages: (paneId, args) => bridge.readOpencodeMessages(paneId, args),
    replyOpencodePermission: (paneId, requestId, reply) =>
      bridge.replyOpencodePermission(paneId, requestId, reply),
    replyOpencodeQuestion: (paneId, args) => bridge.replyOpencodeQuestion(paneId, args),
  };
}
