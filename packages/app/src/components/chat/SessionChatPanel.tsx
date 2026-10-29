// The chat over a terminal pane: reads the pane's session file while it is mounted (TerminalPane mounts it
// only when the person picked Chat). `dock` is what waits on the person, shown under the conversation.
// `reply`: give the chat a message box (the Cockpit's Chat tab does); `blockedReason` switches the box off.
import { useMemo, useState, type ReactNode } from 'react';
import { usePaneOutbox } from '../../hooks/usePaneOutbox';
import { tauriOutboxTransport } from '../../lib/paneOutbox';
import { useComposerDraft } from '../../lib/composerDraft';
import { usePendingBubbles } from '../../hooks/usePendingBubbles';
import { useSessionChat } from '../../hooks/useSessionChat';
import { BranchSheet } from '../cockpit/BranchSheet';
import { BranchBanner } from './BranchBanner';
import { ChatComposer } from './ChatComposer';
import { ChatExportMenu } from './ChatExportMenu';
import { ClipSheet, type ClipApi } from './ClipSheet';
import { KeepGoingBanner } from './KeepGoingBanner';
import { PendingMessages } from './PendingMessages';
import { lastClippableId, SessionChatView } from './SessionChatView';
import { useSideQuestion } from './useSideQuestion';
import { useChatJump } from '../../lib/chatJump';

export interface ChatReply {
  /** Why the message box is switched off, or null. */
  blockedReason: string | null;
}

export function SessionChatPanel({ paneId, dock, reply, emptyNote, clipApi, onOpenPane }: { paneId: string; dock?: ReactNode; reply?: ChatReply; emptyNote?: ReactNode; clipApi?: ClipApi; onOpenPane?: (paneId: string) => void }) {
  const chat = useSessionChat(paneId, true);
  const outbox = usePaneOutbox(tauriOutboxTransport, paneId, !!reply);
  const userTexts = useMemo(() => chat.turns.flatMap((turn) => (turn.user ? [turn.user.text] : [])), [chat.turns]);
  const { bubbles, hide } = usePendingBubbles(outbox.messages, userTexts);
  const side = useSideQuestion(paneId, chat.turns);
  const jump = useChatJump(paneId);
  // "Make clip": the id of the message that started the turn the sheet is open for, or null.
  const [clipFor, setClipFor] = useState<string | null>(null);
  // "Branch": the id of the message that started the turn the branch sheet is open for, or null.
  const [branchFor, setBranchFor] = useState<string | null>(null);
  const hasChat = chat.state !== 'unavailable';
  // A chat with no message box is read-only (the terminal pane's chat): no Branch there.
  const canBranch = hasChat && !!reply;
  const lastClippable = useMemo(() => lastClippableId(chat.turns), [chat.turns]);
  // The Point and Fix draft ('Edit in Chat') for this pane's box: taken only while the box is shown.
  const draft = useComposerDraft(reply ? paneId : null);
  return (
    <>
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
        headerMenu={hasChat ? <ChatExportMenu paneId={paneId} onMakeClip={lastClippable ? () => setClipFor(lastClippable) : undefined} /> : undefined}
        banner={
          <>
            <KeepGoingBanner paneId={paneId} onOpenPane={onOpenPane} />
            <BranchBanner paneId={paneId} onOpenPane={onOpenPane} />
          </>
        }
        canClip={hasChat}
        onMakeClip={setClipFor}
        canBranch={canBranch}
        onBranch={setBranchFor}
        overlay={side.box}
        dock={dock}
        pending={reply ? <PendingMessages messages={bubbles} onCancel={outbox.cancel} onHide={hide} /> : undefined}
        composer={reply ? <ChatComposer onSend={outbox.send} blockedReason={reply.blockedReason} draftKey={paneId} draft={draft} /> : undefined}
      />
      {clipFor !== null && <ClipSheet paneId={paneId} userItemId={clipFor} api={clipApi} onClose={() => setClipFor(null)} />}
      {branchFor !== null && (
        <BranchSheet paneId={paneId} from={{ kind: 'message', user_item_id: branchFor, when: 'after' }} onClose={() => setBranchFor(null)} />
      )}
    </>
  );
}
