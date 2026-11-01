// Moving a conversation between its chat and a terminal pane. The backend either does it or says "ask the person first"
// (the agent is working); the person's yes calls it again with `force`. On success the other pane is selected.
import { useCallback, useEffect, useState } from 'react';
import { errorSentence, handOverAsk, tauriAcpChatApi, type AcpChatApi, type HandOverKind } from '../lib/acpChat';

export interface HandOverAsk {
  kind: HandOverKind;
  conversation: string;
  /** The backend's own sentence, e.g. "The agent is working in the chat". */
  text: string;
}

export function useHandOver({ api = tauriAcpChatApi, select, notify, resetOn }: { api?: AcpChatApi; select: (paneId: string) => void; notify: (text: string) => void; /** A question left open is dropped when this changes (the pane on screen). */ resetOn?: string }) {
  const [ask, setAsk] = useState<HandOverAsk | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => setAsk(null), [resetOn]);

  const run = useCallback(
    async (kind: HandOverKind, conversation: string, force = false) => {
      setBusy(true);
      try {
        if (kind === 'toTerminal') {
          const reply = await api.openInTerminal(conversation, force);
          const wait = handOverAsk(reply);
          if (wait) setAsk({ kind, conversation, text: wait.ask });
          else if (reply.ok) {
            setAsk(null);
            select(reply.result.pane_id);
          }
        } else {
          const reply = await api.backToChat(conversation, force);
          const wait = handOverAsk(reply);
          if (wait) setAsk({ kind, conversation, text: wait.ask });
          else {
            setAsk(null);
            select(conversation);
          }
        }
      } catch (e) {
        setAsk(null);
        notify(errorSentence(e));
      } finally {
        setBusy(false);
      }
    },
    [api, select, notify],
  );

  return {
    busy,
    ask,
    start: (kind: HandOverKind, conversation: string) => void run(kind, conversation),
    /** The person said yes to the question: do it, stopping what runs. */
    confirm: () => {
      if (ask) void run(ask.kind, ask.conversation, true);
    },
    cancel: () => setAsk(null),
  };
}
