// A chat session over the Agent Client Protocol (the "Chat (ACP)" way to run an agent): the typed commands, the state
// event, and the pure rules the views use. The conversation itself is the ordinary session chat (`sessionChat.ts`,
// source `acp`); a question the agent asks is an ordinary question card (kind `acp_permission`). This file is only
// what is new: stop, the session's state, and which cards belong to a pane.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Question } from './questions';

/** `PaneInfo.lane` of a chat session. */
export const ACP_LANE = 'acp';
/** `Question.kind` of the card for the agent's own permission request. */
export const ACP_PERMISSION_KIND = 'acp_permission';
/** The badge on the pane (the name of the way to run, never an agent's product name). */
export const ACP_BADGE = 'Chat (ACP)';

/** One permission question of the agent, as the session reports it. */
export interface AcpPermission {
  askId: number;
  title: string | null;
  toolKind: string | null;
  /** The question card that stands for it (absent for a moment before it is raised). */
  questionId: string | null;
  options: { id: string; name: string; kind: string }[] | null;
  sinceMs: number;
}

/** The session behind a chat pane (Rust `AcpChat::state`). */
export interface AcpChatState {
  lane: 'acp';
  agent: string;
  label: string;
  /** starting, idle, working, needs_input, cancelling, resuming, dormant, exited. */
  phase: string;
  status: 'running' | 'waiting_input' | 'idle' | 'error';
  turnInFlight: boolean;
  /** Stop was asked and the agent has not ended the turn yet. */
  stopping: boolean;
  /** The agent takes text while it works (a message is then sent into the running turn). */
  canSteer: boolean;
  turn: number;
  title: string | null;
  usage: { used: number; size: number } | null;
  permissions: AcpPermission[];
  closed: boolean;
}

export interface AcpChatApi {
  state: (paneId: string) => Promise<AcpChatState | null>;
  cancel: (paneId: string) => Promise<boolean>;
  watch: (cb: (paneId: string, state: AcpChatState) => void) => Promise<UnlistenFn>;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const noopUnlisten: UnlistenFn = () => {};

export const tauriAcpChatApi: AcpChatApi = {
  state: async (paneId) => {
    if (!isTauri()) return null;
    return (await invoke<AcpChatState | null>('acp_chat_state', { paneId })) ?? null;
  },
  cancel: async (paneId) => {
    if (!isTauri()) throw new Error('Desktop shell required: acp_chat_cancel');
    return invoke<boolean>('acp_chat_cancel', { paneId });
  },
  watch: async (cb) => {
    if (!isTauri()) return noopUnlisten;
    return listen<{ paneId: string; state: AcpChatState }>('acp-chat-state', (e) => cb(e.payload.paneId, e.payload.state));
  },
};

/** The permission cards that belong to `paneId`, oldest first. */
export function permissionCards(questions: readonly Question[], paneId: string): Question[] {
  return questions.filter((q) => q.kind === ACP_PERMISSION_KIND && q.pane_id === paneId && q.state === 'pending');
}

/**
 * What the strip above the message box says and does. `null`: nothing runs, no strip. `asked`: the person pressed
 * Stop and the backend's state has not come back yet, so the strip already reads "Stopping…".
 */
export function runStrip(state: AcpChatState | null, asked = false): { text: string; stopping: boolean } | null {
  if (!state || state.closed || !state.turnInFlight) return null;
  if (state.stopping || asked) return { text: 'Stopping…', stopping: true };
  if (state.phase === 'needs_input') return { text: 'Waiting for your answer', stopping: false };
  return { text: 'Working…', stopping: false };
}
