// Typed wrappers for exactly the commands and events in docs/design/cockpit-contract.md.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

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

  onModeChanged: (cb: (e: { mode: UiMode }) => void) => on('ui://mode-changed', cb),
  onSelect: (cb: (e: { pane_id: string }) => void) => on('cockpit://select', cb),
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
  onStandbyChanged: (cb: (e: unknown) => void) => on('standby://ready-changed', cb),

  openSettings: (tab?: string) => call<void>('open_settings', { tab }),
};
