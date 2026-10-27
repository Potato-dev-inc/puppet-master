// The chat's view mode: Summary (the person's messages, the final answers and the activity lines), Normal (the
// default: activity lines collapsed) or Verbose (every row open with its detail). Remembered per pane in localStorage.
import { useState } from 'react';

export type ChatViewMode = 'summary' | 'normal' | 'verbose';

export const CHAT_VIEW_MODES: readonly ChatViewMode[] = ['summary', 'normal', 'verbose'];

export const CHAT_VIEW_LABEL: Record<ChatViewMode, string> = {
  summary: 'Summary',
  normal: 'Normal',
  verbose: 'Verbose',
};

/** Anything that is not a known mode (an old or broken value) is Normal. */
export function parseChatViewMode(value: unknown): ChatViewMode {
  return CHAT_VIEW_MODES.includes(value as ChatViewMode) ? (value as ChatViewMode) : 'normal';
}

const storageKey = (paneId: string) => `pm.chatView.${paneId}`;

/** The pane's saved mode. No pane, or no storage (a private window, blocked site data): Normal. */
export function readChatViewMode(paneId: string | null | undefined): ChatViewMode {
  if (!paneId) return 'normal';
  try {
    return parseChatViewMode(localStorage.getItem(storageKey(paneId)));
  } catch {
    return 'normal';
  }
}

export function writeChatViewMode(paneId: string | null | undefined, mode: ChatViewMode): void {
  if (!paneId) return;
  try {
    localStorage.setItem(storageKey(paneId), mode);
  } catch {
    // Not saved: the mode still holds until the chat is closed.
  }
}

/** The mode of a pane's chat and a setter that also remembers it. A new pane reads its own saved mode. */
export function useChatViewMode(paneId: string | null | undefined): [ChatViewMode, (mode: ChatViewMode) => void] {
  const pane = paneId ?? null;
  const [state, setState] = useState(() => ({ pane, mode: readChatViewMode(pane) }));
  let current = state;
  if (state.pane !== pane) {
    current = { pane, mode: readChatViewMode(pane) };
    setState(current);
  }
  const setMode = (mode: ChatViewMode) => {
    writeChatViewMode(pane, mode);
    setState({ pane, mode });
  };
  return [current.mode, setMode];
}
