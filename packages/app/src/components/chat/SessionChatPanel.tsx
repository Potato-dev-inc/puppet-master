// The chat over a terminal pane: reads the pane's session file while it is mounted (TerminalPane mounts it
// only when the person picked Chat). `dock` is what waits on the person, shown under the conversation.
// `reply`: give the chat a message box (the Cockpit's Chat tab does); `blockedReason` switches the box off.
import { useMemo, type ReactNode } from 'react';
import { usePaneOutbox } from '../../hooks/usePaneOutbox';
import { tauriOutboxTransport } from '../../lib/paneOutbox';
import { usePendingBubbles } from '../../hooks/usePendingBubbles';
import { useSessionChat } from '../../hooks/useSessionChat';
import { ChatComposer } from './ChatComposer';
import { ChatExportMenu } from './ChatExportMenu';
import { PendingMessages } from './PendingMessages';
import { SessionChatView } from './SessionChatView';
import { useSideQuestion } from './useSideQuestion';
import { useChatJump } from '../../lib/chatJump';

export interface ChatReply {
  /** Why the message box is switched off, or null. */
  blockedReason: string | null;
}

export function SessionChatPanel({ paneId, dock, reply, emptyNote }: { paneId: string; dock?: ReactNode; reply?: ChatReply; emptyNote?: ReactNode }) {
  const chat = useSessionChat(paneId, true);
  const outbox = usePaneOutbox(tauriOutboxTransport, paneId, !!reply);
  const userTexts = useMemo(() => chat.turns.flatMap((turn) => (turn.user ? [turn.user.text] : [])), [chat.turns]);
  const { bubbles, hide } = usePendingBubbles(outbox.messages, userTexts);
  const side = useSideQuestion(paneId, chat.turns);
  const jump = useChatJump(paneId);
  return (
    <SessionChatView
      state={chat.state}
      reason={chat.reason}
      waitingFor={chat.waitingFor}
      source={chat.source}
      turns={chat.turns}
      model={chat.model}
      cliVersion={chat.cliVersion}
      paneId={paneId}
      jump={jump}
      remember
      emptyNote={emptyNote}
      headerExtra={chat.state === 'unavailable' ? undefined : side.button}
      headerMenu={chat.state === 'unavailable' ? undefined : <ChatExportMenu paneId={paneId} />}
      overlay={side.box}
      dock={dock}
      pending={reply ? <PendingMessages messages={bubbles} onCancel={outbox.cancel} onHide={hide} /> : undefined}
      composer={reply ? <ChatComposer onSend={outbox.send} blockedReason={reply.blockedReason} draftKey={paneId} /> : undefined}
    />
  );
}
