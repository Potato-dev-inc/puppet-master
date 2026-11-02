// The chat over a terminal pane: reads the pane's session file while it is mounted (TerminalPane mounts it
// only when the person picked Chat). `dock` is what waits on the person, shown under the conversation.
// `reply`: give the chat a message box (the Cockpit's Chat tab does); `blockedReason` switches the box off.
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { keepGoingContinue, keepGoingTargetKey, keepGoingTargets } from '../../lib/keep-going';
import { openSettingsAt, SETUP_CHECK_ROW_ID } from '../../lib/openSettingsAt';
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
import { CollisionBanner } from './CollisionBanner';
import { KeepGoingBanner, plainKeepGoingError } from './KeepGoingBanner';
import type { EndingActions } from './TurnEndingButtons';
import { PendingMessages } from './PendingMessages';
import { ScreenQuestionCard } from './ScreenQuestionCard';
import { lastClippableId, SessionChatView } from './SessionChatView';
import { useSideQuestion } from './useSideQuestion';
import { useChatJump } from '../../lib/chatJump';
import type { ChatHelper } from '../../lib/chat-helpers';
import type { AcpCommand } from '../../lib/acpChat';

export interface ChatReply {
  /** Why the message box is switched off, or null. */
  blockedReason: string | null;
  /** The agent is working: the box shows Stop instead of Send (see ChatComposer). */
  running?: boolean;
  /** Stops the agent. Set for a Chat (terminal) pane only: a chat session over ACP has its own Stop (AcpChatDock). */
  onStop?: () => void;
  /**
   * Stops the running turn so a waiting message can go at once ("Send now"). Set for a chat session and a Chat
   * (terminal) pane; without it a waiting message has Cancel only.
   */
  onInterrupt?: () => void;
}

/**
 * The other agent a "usage limit" ending can hand the task to: the first one `keep_going_targets` says is ready.
 * Only asked while the newest turn ended that way. Null while unknown, when none is set up, or when the pane cannot
 * hand over (the command rejects).
 */
function useKeepGoingOffer(paneId: string, wanted: boolean): EndingActions['keepGoing'] {
  const [offer, setOffer] = useState<EndingActions['keepGoing']>(null);
  useEffect(() => {
    if (!wanted) {
      setOffer(null);
      return;
    }
    let live = true;
    keepGoingTargets(paneId).then(
      (offers) => {
        const ready = offers.find((o) => o.ok);
        if (!live) return;
        setOffer(
          ready
            ? {
                label: ready.label,
                run: () =>
                  keepGoingContinue(paneId, keepGoingTargetKey(ready.target), false).then(
                    () => null,
                    (err: unknown) => plainKeepGoingError(err, 'Could not move the task. Try again.'),
                  ),
              }
            : null,
        );
      },
      () => {
        if (live) setOffer(null);
      },
    );
    return () => {
      live = false;
    };
  }, [paneId, wanted]);
  return offer;
}

/**
 * What the chat needs to show "Agent is asking" for a question that only the terminal screen holds (see
 * `ScreenQuestionCard`). Without it the chat never reads the screen.
 */
export interface ScreenAsk {
  /** The pane's status is waiting_input. */
  waiting: boolean;
  /** The pane's status is idle. */
  idle: boolean;
  /** The pane's status is running (its screen is read for a menu that holds still). */
  running?: boolean;
  /** An approval or question card already answers the pane: nothing is added. */
  covered: boolean;
  fallbackText?: string | null;
  onOpenTerminal?: () => void;
  /** The agent's name when this Chat (terminal) pane's agent can answer prompts: the answerable card is drawn. */
  answerable?: string | null;
}

export function SessionChatPanel({ paneId, dock, reply, emptyNote, clipApi, onOpenPane, screenAsk, helpers, slashCommands, composerTop }: { paneId: string; dock?: ReactNode; reply?: ChatReply; emptyNote?: ReactNode; clipApi?: ClipApi; onOpenPane?: (paneId: string) => void; screenAsk?: ScreenAsk; /** The helpers this pane started through Puppet Master (rows in the turn they began in). */ helpers?: readonly ChatHelper[]; /** A chat session over ACP: the agent's slash commands for the message box (typing "/" opens a menu). */ slashCommands?: readonly AcpCommand[]; /** Drawn right above the message box (a chat session's settings chips). */ composerTop?: ReactNode }) {
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
  // The buttons on the newest turn's ending line.
  const newest = chat.turns[chat.turns.length - 1];
  const keepGoing = useKeepGoingOffer(paneId, hasChat && !!reply && newest?.ending?.reason === 'usage_limit');
  const newestText = newest?.user?.text;
  const endings: EndingActions | undefined = hasChat
    ? {
        onRetry: reply && newestText ? () => outbox.send(newestText) : undefined,
        onSignIn: () => openSettingsAt('agents', SETUP_CHECK_ROW_ID),
        keepGoing,
      }
    : undefined;
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
        helpers={helpers}
        onOpenPane={onOpenPane}
        endings={endings}
        remember
        emptyNote={emptyNote}
        headerExtra={chat.state === 'unavailable' ? undefined : side.button}
        headerMenu={hasChat ? <ChatExportMenu paneId={paneId} onMakeClip={lastClippable ? () => setClipFor(lastClippable) : undefined} /> : undefined}
        banner={
          <>
            <CollisionBanner paneId={paneId} onOpenPane={onOpenPane} />
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
        aside={
          screenAsk && (
            <ScreenQuestionCard
              paneId={paneId}
              waiting={screenAsk.waiting}
              idle={screenAsk.idle}
              running={screenAsk.running}
              covered={screenAsk.covered}
              queued={outbox.messages}
              fallbackText={screenAsk.fallbackText}
              onOpenTerminal={screenAsk.onOpenTerminal}
              answerable={screenAsk.answerable}
            />
          )
        }
        pending={reply ? <PendingMessages messages={bubbles} onCancel={outbox.cancel} onHide={hide} onSendNow={reply.onInterrupt} /> : undefined}
        composer={reply ? (
          <ChatComposer onSend={outbox.send} blockedReason={reply.blockedReason} draftKey={paneId} draft={draft} slashCommands={slashCommands} toolbar={composerTop} running={reply.running} onStop={reply.onStop} />
        ) : undefined}
      />
      {clipFor !== null && <ClipSheet paneId={paneId} userItemId={clipFor} api={clipApi} onClose={() => setClipFor(null)} />}
      {branchFor !== null && (
        <BranchSheet paneId={paneId} from={{ kind: 'message', user_item_id: branchFor, when: 'after' }} onClose={() => setBranchFor(null)} />
      )}
    </>
  );
}
