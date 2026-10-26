// Read-only chat of one terminal pane: opens the backend's session-file reader while `enabled`, merges the
// item batches for this pane, and closes the reader again on unmount or when disabled.
import { useEffect, useMemo, useState } from 'react';
import {
  closeSessionChat,
  groupTurns,
  mergeItems,
  onSessionChatItems,
  openSessionChat,
  type ChatItem,
  type ChatTurn,
  type UsageItem,
} from '../lib/sessionChat';

export type SessionChatState = 'loading' | 'ready' | 'unavailable';

export interface SessionChatHook {
  state: SessionChatState;
  /** Why the chat is unavailable (from the backend, or the error that stopped the open). */
  reason: string | null;
  turns: ChatTurn[];
  model: string | null;
  cliVersion: string | null;
  usage: UsageItem | null;
}

interface Status {
  state: SessionChatState;
  reason: string | null;
}

const LOADING: Status = { state: 'loading', reason: null };

export function useSessionChat(paneId: string, enabled: boolean): SessionChatHook {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [status, setStatus] = useState<Status>(LOADING);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let unlisten: (() => void) | null = null;
    setItems([]);
    setStatus(LOADING);

    // Subscribe before opening, so no batch that arrives while the open is in flight is lost.
    void onSessionChatItems((event) => {
      if (!live || event.paneId !== paneId) return;
      setItems((prev) => mergeItems(prev, event.items, event.reset));
    })
      .then((off) => {
        if (live) unlisten = off;
        else off();
      })
      .catch(() => undefined);

    openSessionChat(paneId)
      .then((snapshot) => {
        if (!live) return;
        setItems((prev) => mergeItems(prev, snapshot.items, false));
        setStatus(snapshot.available ? { state: 'ready', reason: null } : { state: 'unavailable', reason: snapshot.reason });
      })
      .catch((error: unknown) => {
        if (!live) return;
        setStatus({ state: 'unavailable', reason: error instanceof Error ? error.message : String(error) });
      });

    return () => {
      live = false;
      unlisten?.();
      void closeSessionChat(paneId).catch(() => undefined);
    };
  }, [paneId, enabled]);

  const grouped = useMemo(() => groupTurns(items), [items]);
  return {
    state: status.state,
    reason: status.reason,
    turns: grouped.turns,
    model: grouped.model,
    cliVersion: grouped.cliVersion,
    usage: grouped.usage,
  };
}
