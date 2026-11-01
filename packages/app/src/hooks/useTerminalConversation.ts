// Does this ordinary terminal pane hold a chat conversation (one that was handed over from its chat)? Asked once when the
// pane opens and again after every hand-over (`acp-conversations-changed`). `null` for a pane that holds none, while
// unknown, and without a desktop shell.
import { useEffect, useState } from 'react';
import { tauriAcpChatApi, type AcpChatApi, type TerminalConversation } from '../lib/acpChat';

export function useTerminalConversation(paneId: string, enabled: boolean, api: AcpChatApi = tauriAcpChatApi): TerminalConversation | null {
  const [found, setFound] = useState<TerminalConversation | null>(null);

  useEffect(() => {
    setFound(null);
    if (!enabled) return;
    let live = true;
    let off: (() => void) | null = null;
    // Asked after the first read and after every hand-over; a late answer for an older pane is dropped by `live`.
    const ask = () => {
      api
        .conversationOfTerminal(paneId)
        .then((next) => live && setFound(next ?? null))
        .catch(() => live && setFound(null));
    };
    api
      .watchConversations(ask)
      .then((stop) => {
        if (live) off = stop;
        else stop();
      })
      .catch(() => undefined);
    ask();
    return () => {
      live = false;
      off?.();
    };
  }, [paneId, enabled, api]);

  return found;
}
