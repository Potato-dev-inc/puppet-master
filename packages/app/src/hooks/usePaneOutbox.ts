// The message box of one pane's Chat tab: the pane's waiting and just-finished messages (the list from the
// transport, then its change events), and the two things a person does: send and cancel. The transport is the
// desktop's Tauri commands or the phone's bridge (lib/paneOutbox.ts, lib/paneOutbox-bridge.ts).
import { useCallback, useEffect, useState } from 'react';
import { errorText, withSent, type OutboxMessage, type OutboxTransport } from '../lib/paneOutbox';

export interface PaneOutboxHook {
  messages: OutboxMessage[];
  /** Sends a message. Resolves to null, or to one plain sentence saying why it was not accepted. */
  send: (text: string) => Promise<string | null>;
  /** Cancels a waiting message. Resolves to null, or to the reason it could not be cancelled. */
  cancel: (messageId: string) => Promise<string | null>;
}

const NOT_CONNECTED = 'Could not reach your PC.';

export function usePaneOutbox(transport: OutboxTransport | null, paneId: string, enabled: boolean): PaneOutboxHook {
  const [messages, setMessages] = useState<OutboxMessage[]>([]);

  useEffect(() => {
    if (!enabled || !transport) return;
    let live = true;
    setMessages([]);
    // Watch before listing, so a change that happens while the list is in flight is not lost.
    const stop = transport.watch(paneId, (list) => {
      if (live) setMessages(list);
    });
    transport
      .list(paneId)
      .then((list) => {
        // An event that came first is newer than this list.
        if (live) setMessages((prev) => (prev.length > 0 ? prev : list));
      })
      .catch(() => undefined);
    return () => {
      live = false;
      stop();
    };
  }, [transport, paneId, enabled]);

  const send = useCallback(
    async (text: string) => {
      if (!transport) return NOT_CONNECTED;
      try {
        const sent = await transport.send(paneId, text);
        setMessages((prev) => withSent(prev, sent));
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [transport, paneId],
  );

  const cancel = useCallback(
    async (messageId: string) => {
      if (!transport) return NOT_CONNECTED;
      try {
        const done = await transport.cancel(paneId, messageId);
        setMessages((prev) => prev.map((m) => (m.id === done.id ? done : m)));
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [transport, paneId],
  );

  return { messages, send, cancel };
}
