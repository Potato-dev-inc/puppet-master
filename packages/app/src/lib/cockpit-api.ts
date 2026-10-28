// Typed wrappers for exactly the commands and events in docs/design/cockpit-contract.md.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { UserGesture } from './user-gesture';
import type { PackageList } from './package-check';

export type UiMode = 'orchestration' | 'individual';

export interface PromptChoice {
  id: string;
  label: string;
  /** "Don't ask again" / "allow all" style answer (set by the backend; older builds omit it). */
  broad?: boolean;
}

export type RiskLevel = 'low' | 'medium' | 'high';

/** Plain-language risk of what saying yes would do (backend `prompt_risk`). */
export interface PromptRisk {
  level: RiskLevel;
  headline: string;
  reasons: string[];
}

export interface PanePrompt {
  promptId: string;
  kind: string;
  text: string;
  summary?: string;
  choices: PromptChoice[];
  /** Absent on older backends: the UI then shows the plain question. */
  risk?: PromptRisk;
  /** Package check: the packages this command installs. Absent when it installs none. */
  packages?: PackageList;
}

export interface PaneRunInfo {
  handle: string;
  status: string;
  receipt: unknown | null;
  /** Why the run failed: the typed code (and the backend's message). Absent while it did not fail. */
  error?: { code: string; message?: string } | null;
}

export interface PaneTranscript {
  chunks: { stream: string; text: string }[];
}

export interface WorktreeItem {
  handle: string;
  branch: string;
  base: string;
  /** Origin project the worktree belongs to. */
  repo: string;
  path: string;
  exists: boolean;
  /** `active`: an agent is still working in it. `unreviewed`: finished, not merged or discarded. */
  state: 'active' | 'unreviewed' | 'missing';
  summary: { files_changed: number; additions: number; deletions: number } | null;
  /** The agent type that works in it (for "Send back to <agent>"). */
  agent_type?: string | null;
  /** What preparing the copy did (setup steps, files copied in). */
  setup?: import('./ship-api').SetupReport | null;
  /** The pull request made from this copy, with the last result of its GitHub checks. */
  pr?: import('./ship-api').PrRef | null;
}

/** Who last changed a file in a project folder, from its turn summaries (`changes_blame`). */
export interface ChangeBlame {
  /** The agent's display name ("Codex"). */
  agent: string;
  /** The pane's title when the pane is still open. */
  paneTitle: string | null;
  /** When the turn that changed the file ended (epoch ms). */
  atMs: number;
}

export interface WorktreeFile {
  path: string;
  status: 'added' | 'modified' | 'deleted';
  additions: number;
  deletions: number;
  binary: boolean;
}

export interface WorktreeDiff {
  branch: string;
  base: string;
  changes: { files: WorktreeFile[]; files_changed: number; additions: number; deletions: number; truncated: boolean };
  patch: string;
  patch_truncated: boolean;
}

export interface WorktreeMergeOutcome {
  merged: boolean;
  reason?: string;
  merge_commit: string | null;
  files: string[];
  branch: string;
  into: string;
}

export type SpawnStage = 'claiming' | 'spawning' | 'attached' | 'ready' | 'failed';

export interface SpawnStageEvent {
  agent_id: string;
  pane_id: string | null;
  stage: SpawnStage;
  warm: boolean;
  message?: string;
}

import type { Question } from './questions';

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
  state: 'ok' | 'off' | 'no_git' | 'too_big';
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
  /** The attention item this line is (a stable key); absent from older builds. */
  id?: string;
  /** Empty for something that has no pane (a question with no pane, a consent card). */
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
  /** A person's answer: `gesture` is their click (`gestureOf(event)`); without one the backend sends
   *  nothing but a refusal of an ordinary prompt (never a yes, never folder trust). */
  answerPanePrompt: (paneId: string, promptId: string, choice: string, gesture: UserGesture) =>
    call<void>('answer_pane_prompt', { paneId, promptId, choice, gesture }),
  /** Puppet Master's own No (Simple mode, a high-risk command): a refusal needs no click. */
  declinePanePrompt: (paneId: string, promptId: string, choice: string) =>
    call<void>('answer_pane_prompt', { paneId, promptId, choice, gesture: null }),
  interruptPane: (paneId: string) => call<void>('interrupt_pane', { paneId }),
  newPane: (agentId: string, accountId?: string, isolation?: 'none' | 'worktree', projectPath?: string) =>
    call<string>('new_pane', {
      agentId,
      accountId: accountId ?? null,
      isolation: isolation ?? null,
      projectPath: projectPath ?? null,
    }),
  paneWorktree: (paneId: string) => call<WorktreeItem | null>('pane_worktree', { paneId }),
  /** The git branch of the folder the pane works in; null for a folder that is not a git work tree. */
  paneGitBranch: (paneId: string) => call<string | null>('pane_git_branch', { paneId }),
  worktreeDiff: (projectPath: string | null, handle: string, path?: string) =>
    call<WorktreeDiff>('worktree_diff', { projectPath, handle, path: path ?? null }),
  worktreeMerge: (projectPath: string | null, handle: string) =>
    call<WorktreeMergeOutcome>('worktree_merge', { projectPath, handle }),
  worktreeDiscard: (projectPath: string | null, handle: string) =>
    call<void>('worktree_discard', { projectPath, handle }),
  paneRunInfo: (paneId: string) => call<PaneRunInfo | null>('pane_run_info', { paneId }),
  paneTranscript: (paneId: string) => call<PaneTranscript | null>('pane_transcript', { paneId }),
  checkpointsList: (paneId: string) => call<CheckpointList>('checkpoints_list', { paneId }),
  checkpointDiff: (paneId: string, id: string) => call<FileChange[]>('checkpoint_diff', { paneId, id }),
  checkpointRestore: (paneId: string, id: string) => call<RestoreReport>('checkpoint_restore', { paneId, id }),
  checkpointUndoLast: (paneId: string) => call<RestoreReport>('checkpoint_undo_last', { paneId }),
  /** Rename a pane; a blank title clears it and automatic titling resumes. */
  paneSetTitle: (paneId: string, title: string) => call<string | null>('pane_set_title', { paneId, title }),
  ambientGetPrefs: () => call<{ animated_tray_icon: boolean }>('ambient_get_prefs'),
  ambientSetAnimatedTray: (enabled: boolean) => call<void>('ambient_set_animated_tray', { enabled }),
  turnSummaries: (paneId: string) => call<TurnSummary[]>('turn_summaries', { paneId }),
  /** The agent (and pane) that last changed each path in `project`; a file no turn touched has no entry. */
  changesBlame: (project: string, paths: string[]) =>
    call<Record<string, ChangeBlame>>('changes_blame', { project, paths }),
  presenceCockpitFocus: (focused: boolean) => call<void>('presence_cockpit_focus', { focused }),
  awayDigestPending: () => call<Digest | null>('away_digest_pending'),
  awayDigestDismiss: () => call<void>('away_digest_dismiss'),

  onModeChanged: (cb: (e: { mode: UiMode }) => void) => on('ui://mode-changed', cb),
  onSelect: (cb: (e: { pane_id: string }) => void) => on('cockpit://select', cb),
  /** Rust asks the cockpit to show Settings inline (tray "Settings" in Orchestration mode). */
  onSettingsOpen: (cb: (e: { tab?: string | null }) => void) => on('cockpit://settings', cb),
  /** Rust asks a visible Settings window to move into the cockpit (the mode switched to Orchestration). */
  onSettingsToInline: (cb: () => void) => on('settings://to-inline', cb),
  // The event is built where the pane's folder is unknown, so its risk cannot say "outside this
  // project". Re-read the prompt through the command (which knows the folder) before showing it.
  onPrompt: (cb: (e: { pane_id: string; prompt: PanePrompt | null }) => void) =>
    on<{ pane_id: string; prompt: PanePrompt | null }>('pty://prompt', (e) => {
      if (!e.prompt) return cb(e);
      const id = e.prompt.promptId;
      call<PanePrompt | null>('pane_prompt', { paneId: e.pane_id })
        .then((fresh) => cb(fresh && fresh.promptId === id ? { pane_id: e.pane_id, prompt: fresh } : e))
        .catch(() => cb(e));
    }),
  onSpawnStage: (cb: (e: SpawnStageEvent) => void) => on('pane://spawn-stage', cb),
  onTurnSummary: (cb: (e: TurnSummary) => void) => on('turn://summary', cb),
  onDigestReady: (cb: (e: Digest) => void) => on('digest://ready', cb),
  onStandbyChanged: (cb: (e: unknown) => void) => on('standby://ready-changed', cb),

  questionsList: () => call<Question[]>('questions_list'),
  answerQuestion: (id: string, answer: { choice_id?: string; text?: string }) =>
    call<{ accepted: boolean; question: Question }>('question_answer', { id, choiceId: answer.choice_id, text: answer.text }),
  dismissQuestion: (id: string) => call<{ accepted: boolean; question: Question }>('question_dismiss', { id }),
  onQuestionsChanged: (cb: () => void) => on('questions://changed', cb),

  openSettings: (tab?: string) => call<void>('open_settings', { tab }),
};
