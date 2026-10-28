// Wire types of the Puppet Master HTTP bridge: projections returned by the Rust side, request/response
// shapes, the BridgeClient interface and the SSE event union. Pure types; no runtime code.
import type {
  AgentContextProfile,
  AgentModelInspection,
  McpLogEntry,
  OrchestratorChatEvent,
  PaneInfo,
} from '@puppet-master/shared';
import type { PublicSettings } from './bridge-settings';
import type { PreviewUrl } from './pane-preview';
import type { AttentionAnswerBody, AttentionAnswerOutcome, AttentionSnapshot } from './attention';
import type { Question, QuestionAnswer } from './questions';
import type { PhoneChatReply } from './sessionChatModel';
import type { OutboxMessage } from './paneOutbox';
import type { CompanionView, FeedItem, MemoryView, PendingDecision } from './companion';
import type { PlansReport } from './plans-report';

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

/** Answer of `POST /panes/{id}/viewport`. `held`: whether this viewer holds the pane's size now. */
export interface PaneViewportResponse {
  ok?: boolean;
  leased?: boolean;
  held?: boolean;
  cols?: number;
  rows?: number;
  restored?: { cols: number; rows: number } | null;
}

/** Body of `GET /team/cards`; `can` is what this phone's pairing allows. */
export interface TeamCardsReply {
  enabled: boolean;
  active: boolean;
  missing: number;
  error: string | null;
  cards: import('./team').TeamCard[];
  jobs: import('./team').TeamJob[];
  can: { dismiss: boolean; act: boolean };
}

export interface BridgeClient {
  baseUrl: string;
  listPanes(): Promise<PaneInfo[]>;
  /** Dev-server URLs per pane (display only; ports are never proxied). Absent on old bridges. */
  listPreviews?(): Promise<Record<string, PreviewUrl[]>>;
  spawnPane(args: {
    agent_type: string;
    cwd?: string;
    cols?: number;
    rows?: number;
    pane_id?: string;
    /** A phone spawning the pane: it holds the pane's size (`cols`x`rows`) from the start. */
    viewer?: string;
  }): Promise<{ pane_id: string }>;
  killPane(paneId: string): Promise<void>;
  /** Panic button: interrupt every running agent pane. */
  stopAllPanes(): Promise<{ stopped: { pane_id: string; agent_type: string; cwd: string; checkpoint_id?: string }[]; failed: [string, string][] }>;
  /** Pending `ask_user` questions. */
  listQuestions(): Promise<Question[]>;
  /** The one list of things that need the person (`GET /attention`). Absent on old bridges. */
  attention?(): Promise<AttentionSnapshot>;
  /**
   * A paired person answers one item of the attention list (`POST /attention/{id}/answer`). Resolves with
   * what the PC did (answered, already answered, changed, gone, failed); rejects with an
   * `AttentionAnswerError` when it refuses the request, or with the network error when it never got there.
   */
  answerAttention?(itemId: string, body: AttentionAnswerBody, signal?: AbortSignal): Promise<AttentionAnswerOutcome>;
  answerQuestion(id: string, answer: QuestionAnswer): Promise<{ accepted: boolean }>;
  dismissQuestion(id: string): Promise<{ accepted: boolean }>;
  /**
   * `POST /panes/{id}/viewport`: while `active`, the pane is sized to this phone's grid (renew
   * every few seconds or it lapses); `active: false` releases this phone's hold (the pane keeps the
   * size). `renew_only: true` only extends an existing hold and never resizes; its `held` answer says
   * whether this phone still holds the pane (false: the desktop took it back).
   */
  setPaneViewport(
    paneId: string,
    view: { cols: number; rows: number; active: boolean; viewer?: string; renew_only?: boolean },
  ): Promise<PaneViewportResponse>;
  /** `GET /agents/types`: the registered agents (body of the desktop `agents_list`). */
  listAgentTypes(): Promise<unknown>;
  /** `GET /digest`: what happened since `since` (default: this device's last visit); `seen` stores now as the last visit. */
  digest?(opts?: { since?: number; seen?: boolean }): Promise<import('./cockpit-api').Digest & { empty: boolean; previousSeenMs: number | null }>;
  /** `GET /team/cards` (needs `watch`): teammates' updates and my queued ones. Absent on old bridges. */
  teamCards?(): Promise<TeamCardsReply>;
  /** The team action routes (`/team/cards/{id}/dismiss|pull|queue`, `/team/jobs/{id}/apply|discard`). */
  teamDismiss?(cardId: string): Promise<{ ok: boolean }>;
  teamPull?(cardId: string): Promise<{ ok: boolean; result: string }>;
  teamQueue?(cardId: string, paneId: string, when: 'idle' | 'now'): Promise<{ ok: boolean; result: import('./team').TeamJob }>;
  teamApply?(jobId: string): Promise<{ ok: boolean; result: string }>;
  teamDiscard?(jobId: string): Promise<{ ok: boolean }>;
  readBuffer(paneId: string, lines: number): Promise<string>;
  readRawBuffer(paneId: string, lines: number): Promise<number[]>;
  /** `GET /panes/{id}/screen`: the pane redrawn at its current size, for a viewer to replay. */
  readScreen(paneId: string): Promise<{ data: Uint8Array; cols: number; rows: number; offset?: number }>;
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
  /**
   * `GET /panes/{id}/chat`: the read-only chat of an agent pane (needs `watch`). With `since` (the `rev` of the last
   * answer) only what changed. Absent on clients that are not a phone.
   */
  paneChat?(paneId: string, since?: number): Promise<PhoneChatReply>;
  /** What this phone may do (`GET /pair/me`). Absent on clients that are not a phone. */
  pairMe?(): Promise<{ local: boolean; device_id?: string; caps: string[] }>;
  /**
   * `POST /panes/{id}/messages`: a message for the pane's agent, sent when the agent is free (needs `type`). Rejects
   * with the PC's plain reason when it refuses. Absent on clients that are not a phone.
   */
  sendPaneMessage?(paneId: string, text: string): Promise<OutboxMessage>;
  /** `DELETE /panes/{id}/messages/{messageId}`: cancels a message that has not started to be typed (needs `type`). */
  cancelPaneMessage?(paneId: string, messageId: string): Promise<OutboxMessage>;
  /** `GET /panes/{id}/messages`: the pane's waiting messages and the ones that just finished (needs `watch`). */
  listPaneMessages?(paneId: string): Promise<OutboxMessage[]>;
  /** The companion routes (`/companions`, needs `watch`; wake and sleep need `control`). Absent on old PCs. */
  companionList?(): Promise<CompanionView[]>;
  companionGet?(id: string): Promise<CompanionView>;
  companionFeed?(id: string, sinceMs: number): Promise<FeedItem[]>;
  companionMemory?(id: string): Promise<MemoryView>;
  companionDecidePending?(id: string, itemId: string, decision: PendingDecision): Promise<MemoryView>;
  companionWake?(id: string): Promise<CompanionView>;
  companionSleep?(id: string): Promise<CompanionView>;
  /** "Your plans" (`GET /plans?month=2026-10`, read only). Absent on old PCs. */
  plansReport?(month?: string): Promise<PlansReport>;
  patchSettings(patch: Partial<PublicSettings>): Promise<PublicSettings>;
  postOrchestratorMessage(text: string, messageId: string): Promise<void>;
  postOrchestratorViewport(viewport: {
    width: number;
    height: number;
    active: boolean;
  }): Promise<void>;
}

/** What the phone shows for a stuck agent (a subset of the loop guard's signal). */
export interface LoopSignalInfo {
  kind: string;
  summary: string;
  evidence_lines?: string[];
}

export type BridgeEvent =
  | { type: 'panes'; panes: PaneInfo[] }
  | { type: 'log'; entry: McpLogEntry }
  | { type: 'chat'; event: OrchestratorChatEvent }
  /** `offset`: where the chunk starts in the pane's byte stream (absent from older backends). */
  | { type: 'terminal'; pane_id: string; data: Uint8Array | number[]; offset?: number }
  | { type: 'terminal-snapshot'; pane_id: string; snapshot: string }
  | { type: 'pane-status'; pane_id: string; status: PaneInfo['status'] }
  | { type: 'pane-preview'; pane_id: string; urls: PreviewUrl[] }
  | { type: 'opencode-worker'; event: OpenCodeWorkerEvent }
  /** `offset`: the stream position the resize falls after (absent from older backends). */
  | { type: 'pane-resize'; pane_id: string; cols: number; rows: number; offset?: number }
  /** A pane looks stuck (`signal`) or the warning cleared (`null`). Sent by the loop guard. */
  | { type: 'loop-signal'; pane_id: string; signal: LoopSignalInfo | null }
  | { type: 'settings'; settings: PublicSettings }
  | { type: 'orchestrator-viewport'; width: number; height: number; active: boolean }
  /** Pending `ask_user` questions changed; refetch the list. `revision` increases with each change. */
  | { type: 'questions'; revision: number }
  /** The full attention snapshot (what needs the person); apply it only when its `rev` is newer. */
  | { type: 'attention'; snapshot: AttentionSnapshot }
  /** The PC refused typed input over the socket (`input-error` frame): `error` is its reason. */
  | { type: 'input-refused'; pane_id: string; error: string }
  /** Long-poll only: events were missed (phone asleep, app restarted); re-read pane state. */
  /** The pane's message box changed (watch only, text already redacted): the whole list of the pane. */
  | { type: 'pane-outbox'; paneId: string; messages: OutboxMessage[] }
  /** Every companion changed (SPEC-1028 section 11): the phone's list, without folders. */
  | { type: 'companion'; companions: CompanionView[] }
  | { type: 'resync' };
