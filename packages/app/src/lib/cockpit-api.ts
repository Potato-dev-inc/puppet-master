// Typed wrappers for exactly the commands and events in docs/design/cockpit-contract.md.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type UiMode = 'orchestration' | 'individual';

export interface PromptChoice {
  id: string;
  label: string;
}

export interface PanePrompt {
  promptId: string;
  kind: string;
  text: string;
  summary?: string;
  choices: PromptChoice[];
}

export interface PaneRunInfo {
  handle: string;
  status: string;
  receipt: unknown | null;
}

export interface PaneTranscript {
  chunks: { stream: string; text: string }[];
}

export type SpawnStage = 'claiming' | 'spawning' | 'attached' | 'ready' | 'failed';

export interface SpawnStageEvent {
  agent_id: string;
  pane_id: string | null;
  stage: SpawnStage;
  warm: boolean;
  message?: string;
}

export interface Checkpoint {
  id: string;
  commit: string;
  paneId: string;
  turn: number;
  kind: 'before' | 'after' | 'answer' | 'undo' | string;
  label: string;
  note: string;
  atMs: number;
  files: number;
  added: number;
  deleted: number;
}

export interface CheckpointList {
  state: 'ok' | 'off' | 'no_git';
  message: string;
  /** Newest first. */
  items: Checkpoint[];
}

export interface FileChange {
  path: string;
  status: 'A' | 'M' | 'D' | string;
  added: number;
  deleted: number;
  binary: boolean;
}

export interface RestoreReport {
  safetyId: string | null;
  written: number;
  removed: number;
  failed: string[];
}

export interface CheckpointsApi {
  checkpointsList: (paneId: string) => Promise<CheckpointList>;
  checkpointDiff: (paneId: string, id: string) => Promise<FileChange[]>;
  checkpointRestore: (paneId: string, id: string) => Promise<RestoreReport>;
  checkpointUndoLast: (paneId: string) => Promise<RestoreReport>;
}

export interface TurnFileStats {
  count: number;
  added: number;
  deleted: number;
  paths: string[];
}

export interface TurnApproval {
  atMs: number;
  promptId: string;
  prompt: string;
  kind: string;
  decision: "allow" | "deny" | "answer" | string;
}

/** What one agent turn did, built without an LLM (Rust: turn_summary). */
export interface TurnSummary {
  paneId: string;
  agent: string;
  turn: number;
  startedAtMs: number;
  endedAtMs: number;
  durationMs: number;
  /** null = unknown (no checkpoints for this pane). */
  files: TurnFileStats | null;
  approvals: TurnApproval[];
  end: { kind: "idle" | "waiting_input" | "error" | string; prompt?: string };
  receipt?: { checks: string; outcome: string };
  /** One plain sentence, ready to show. */
  sentence: string;
}

export interface DigestLine {
  paneId: string;
  agent: string;
  turns: number;
  sentence: string;
  endedAtMs: number;
  end: "idle" | "waiting_input" | "error" | string;
}

export interface NeedsYou {
  paneId: string;
  agent: string;
  status: "waiting_input" | "error" | string;
  prompt?: string;
}

/** "While you were away": built from turn summaries since the away start. */
export interface Digest {
  sinceMs: number;
  nowMs: number;
  agentsWorked: number;
  turns: number;
  lines: DigestLine[];
  needsYou: NeedsYou[];
}

export interface TurnSummaryApi {
  turnSummaries: (paneId: string) => Promise<TurnSummary[]>;
}

const noop: UnlistenFn = () => {};

export const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Desktop shell required: ${command}`);
  return invoke<T>(command, args);
}

async function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return noop;
  return listen<T>(event, (e) => cb(e.payload));
}

export const cockpitApi = {
  getUiMode: () => call<UiMode>('get_ui_mode'),
  setUiMode: (mode: UiMode) => call<void>('set_ui_mode', { mode }),
  openCockpit: (paneId?: string) => call<void>('open_cockpit', { paneId }),
  panePrompt: (paneId: string) => call<PanePrompt | null>('pane_prompt', { paneId }),
  answerPanePrompt: (paneId: string, promptId: string, choice: string) =>
    call<void>('answer_pane_prompt', { paneId, promptId, choice }),
  interruptPane: (paneId: string) => call<void>('interrupt_pane', { paneId }),
  newPane: (agentId: string) => call<string>('new_pane', { agentId }),
  paneRunInfo: (paneId: string) => call<PaneRunInfo | null>('pane_run_info', { paneId }),
  paneTranscript: (paneId: string) => call<PaneTranscript | null>('pane_transcript', { paneId }),
  checkpointsList: (paneId: string) => call<CheckpointList>('checkpoints_list', { paneId }),
  checkpointDiff: (paneId: string, id: string) => call<FileChange[]>('checkpoint_diff', { paneId, id }),
  checkpointRestore: (paneId: string, id: string) => call<RestoreReport>('checkpoint_restore', { paneId, id }),
  checkpointUndoLast: (paneId: string) => call<RestoreReport>('checkpoint_undo_last', { paneId }),
  turnSummaries: (paneId: string) => call<TurnSummary[]>('turn_summaries', { paneId }),
  presenceCockpitFocus: (focused: boolean) => call<void>('presence_cockpit_focus', { focused }),
  awayDigestPending: () => call<Digest | null>('away_digest_pending'),
  awayDigestDismiss: () => call<void>('away_digest_dismiss'),

  onModeChanged: (cb: (e: { mode: UiMode }) => void) => on('ui://mode-changed', cb),
  onSelect: (cb: (e: { pane_id: string }) => void) => on('cockpit://select', cb),
  onPrompt: (cb: (e: { pane_id: string; prompt: PanePrompt | null }) => void) => on('pty://prompt', cb),
  onSpawnStage: (cb: (e: SpawnStageEvent) => void) => on('pane://spawn-stage', cb),
  onTurnSummary: (cb: (e: TurnSummary) => void) => on('turn://summary', cb),
  onDigestReady: (cb: (e: Digest) => void) => on('digest://ready', cb),
  onStandbyChanged: (cb: (e: unknown) => void) => on('standby://ready-changed', cb),

  openSettings: (tab?: string) => call<void>('open_settings', { tab }),
};
