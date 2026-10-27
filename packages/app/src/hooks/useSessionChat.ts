// Read-only chat of one terminal pane: opens the backend's session-file reader while `enabled`, merges the
// item batches for this pane, and closes the reader again on unmount or when disabled.
import { useEffect, useMemo, useRef, useState } from 'react';
import {
  closeSessionChat,
  groupTurns,
  mergeItems,
  onSessionChatItems,
  onSessionChatSource,
  onSessionChatStatus,
  openSessionChat,
  type ChatItem,
  type ChatSource,
  type ChatTurn,
  type OutputFile,
  type PlanItem,
  type SessionChatState,
  type UsageItem,
} from '../lib/sessionChat';

export type { SessionChatState };

export interface SessionChatHook {
  state: SessionChatState;
  /** Why the chat is unavailable (from the backend, or the error that stopped the open). */
  reason: string | null;
  /** While there is nothing to show: what the reader is waiting for (from the backend), else null. */
  waitingFor: string | null;
  /** Where the chat is read from, once known: `file`, or `screen` for an agent with no session file. */
  source: ChatSource | null;
  turns: ChatTurn[];
  model: string | null;
  cliVersion: string | null;
  usage: UsageItem | null;
  /** The agent's task list now; empty when it made none. */
  plan: PlanItem[];
  /** Files the agent's tool calls touched, in order of first touch. */
  outputs: OutputFile[];
}

interface Status {
  state: SessionChatState;
  reason: string | null;
  waitingFor: string | null;
  source: ChatSource | null;
}

const LOADING: Status = { state: 'loading', reason: null, waitingFor: null, source: null };

export function useSessionChat(paneId: string, enabled: boolean): SessionChatHook {
  // This view's own id for the backend: it holds the pane's reader until this view lets go (not the window's).
  const surfaceRef = useRef<string | null>(null);
  surfaceRef.current ??= `s-${Math.random().toString(36).slice(2, 10)}`;
  const surface = surfaceRef.current;
  const [items, setItems] = useState<ChatItem[]>([]);
  const [status, setStatus] = useState<Status>(LOADING);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let unlisten: (() => void) | null = null;
    let unlistenStatus: (() => void) | null = null;
    let unlistenSource: (() => void) | null = null;
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

    void onSessionChatStatus((event) => {
      if (!live || event.paneId !== paneId) return;
      setStatus((prev) => ({ ...prev, waitingFor: event.waitingFor }));
    })
      .then((off) => {
        if (live) unlistenStatus = off;
        else off();
      })
      .catch(() => undefined);

    void onSessionChatSource((event) => {
      if (!live || event.paneId !== paneId) return;
      setStatus((prev) => (prev.state === 'ready' ? { ...prev, source: event.source } : prev));
    })
      .then((off) => {
        if (live) unlistenSource = off;
        else off();
      })
      .catch(() => undefined);

    openSessionChat(paneId, surface)
      .then((snapshot) => {
        if (!live) return;
        setItems((prev) => mergeItems(prev, snapshot.items, false));
        setStatus(
          snapshot.available
            ? { state: 'ready', reason: null, waitingFor: snapshot.waitingFor ?? null, source: snapshot.source ?? 'file' }
            : { state: 'unavailable', reason: snapshot.reason, waitingFor: null, source: null },
        );
      })
      .catch((error: unknown) => {
        if (!live) return;
        setStatus({
          state: 'unavailable',
          reason: error instanceof Error ? error.message : String(error),
          waitingFor: null,
          source: null,
        });
      });

    return () => {
      live = false;
      unlisten?.();
      unlistenStatus?.();
      unlistenSource?.();
      void closeSessionChat(paneId, surface).catch(() => undefined);
    };
  }, [paneId, enabled, surface]);

  const grouped = useMemo(() => groupTurns(items), [items]);
  return {
    state: status.state,
    reason: status.reason,
    waitingFor: status.waitingFor,
    source: status.source,
    turns: grouped.turns,
    model: grouped.model,
    cliVersion: grouped.cliVersion,
    usage: grouped.usage,
    plan: grouped.plan,
    outputs: grouped.outputs,
  };
}
