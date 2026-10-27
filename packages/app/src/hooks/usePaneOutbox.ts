// The message box of one pane's Chat tab: the pane's waiting and just-finished messages (the first list from
// `pane_messages_list`, then the `pane-outbox` events), and the two things a person does: send and cancel.
import { useCallback, useEffect, useState } from 'react';
import {
  cancelPaneMessage,
  errorText,
  listPaneMessages,
  onPaneOutbox,
  sendPaneMessage,
  withSent,
  type OutboxMessage,
} from '../lib/paneOutbox';

export interface PaneOutboxHook {
  messages: OutboxMessage[];
  /** Sends a message. Resolves to null, or to one plain sentence saying why it was not accepted. */
  send: (text: string) => Promise<string | null>;
  /** Cancels a waiting message. Resolves to null, or to the reason it could not be cancelled. */
  cancel: (messageId: string) => Promise<string | null>;
}

export function usePaneOutbox(paneId: string, enabled: boolean): PaneOutboxHook {
  const [messages, setMessages] = useState<OutboxMessage[]>([]);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let unlisten: (() => void) | null = null;
    setMessages([]);
    // Subscribe before listing, so a change that happens while the list is in flight is not lost.
    void onPaneOutbox((event) => {
      if (live && event.paneId === paneId) setMessages(event.messages);
    })
      .then((off) => {
        if (live) unlisten = off;
        else off();
      })
      .catch(() => undefined);
    listPaneMessages(paneId)
      .then((list) => {
        // An event that came first is newer than this list.
        if (live) setMessages((prev) => (prev.length > 0 ? prev : list));
      })
      .catch(() => undefined);
    return () => {
      live = false;
      unlisten?.();
    };
  }, [paneId, enabled]);

  const send = useCallback(
    async (text: string) => {
      try {
        const sent = await sendPaneMessage(paneId, text);
        setMessages((prev) => withSent(prev, sent));
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [paneId],
  );

  const cancel = useCallback(
    async (messageId: string) => {
      try {
        const done = await cancelPaneMessage(paneId, messageId);
        setMessages((prev) => prev.map((m) => (m.id === done.id ? done : m)));
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [paneId],
  );

  return { messages, send, cancel };
}
