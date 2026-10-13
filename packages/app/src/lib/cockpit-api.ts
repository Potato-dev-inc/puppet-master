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
  /** Rename a pane; a blank title clears it and automatic titling resumes. */
  paneSetTitle: (paneId: string, title: string) => call<string | null>('pane_set_title', { paneId, title }),
  ambientGetPrefs: () => call<{ animated_tray_icon: boolean }>('ambient_get_prefs'),
  ambientSetAnimatedTray: (enabled: boolean) => call<void>('ambient_set_animated_tray', { enabled }),

  onModeChanged: (cb: (e: { mode: UiMode }) => void) => on('ui://mode-changed', cb),
  onSelect: (cb: (e: { pane_id: string }) => void) => on('cockpit://select', cb),
  onPrompt: (cb: (e: { pane_id: string; prompt: PanePrompt | null }) => void) => on('pty://prompt', cb),
  onSpawnStage: (cb: (e: SpawnStageEvent) => void) => on('pane://spawn-stage', cb),
  onStandbyChanged: (cb: (e: unknown) => void) => on('standby://ready-changed', cb),

  openSettings: (tab?: string) => call<void>('open_settings', { tab }),
};
