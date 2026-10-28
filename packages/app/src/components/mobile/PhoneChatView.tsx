// The chat of one agent pane on the phone: the same view the desktop has (SessionChatView), fed by
// `GET /panes/{id}/chat?since=<rev>` every 1.5 s while this view and the page are visible. Each answer carries
// only what changed since the last one; `reset` drops the list held. It never touches the pane's size (the
// terminal under it keeps its own lease rules). The message box at the bottom sends through the PC's bridge
// (`paneOutbox-bridge.ts`) and needs the `type` cap; without it the box is replaced by one plain line.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { BridgeClient } from '../../lib/bridge-types';
import { bridgeOutboxTransport } from '../../lib/paneOutbox-bridge';
import { groupTurns, mergeItems, type ChatItem, type ChatSource, type SessionChatState } from '../../lib/sessionChatModel';
import { usePaneOutbox } from '../../hooks/usePaneOutbox';
import { usePendingBubbles } from '../../hooks/usePendingBubbles';
import { ChatComposer } from '../chat/ChatComposer';
import { PendingMessages } from '../chat/PendingMessages';
import { SessionChatView } from '../chat/SessionChatView';
import '../../styles/phone-chat.css';

/** How often the PC is asked while the chat is on screen. */
export const POLL_MS = 1500;
/** Items kept in the list; the oldest go first. The PC sends at most 300 at once. */
const MAX_KEPT = 1500;

export const NO_WATCH_NOTE = 'This phone cannot read chats.';
export const NO_SEND_NOTE = 'This phone cannot send messages.';
const UNREACHABLE = 'Could not reach your PC. Trying again…';

interface Status {
  state: SessionChatState;
  reason: string | null;
  waitingFor: string | null;
  source: ChatSource | null;
}

const LOADING: Status = { state: 'loading', reason: null, waitingFor: null, source: null };

function sameStatus(a: Status, b: Status): boolean {
  return a.state === b.state && a.reason === b.reason && a.waitingFor === b.waitingFor && a.source === b.source;
}

function usePageVisible(): boolean {
  const [visible, setVisible] = useState(() => typeof document === 'undefined' || document.visibilityState !== 'hidden');
  useEffect(() => {
    const onChange = () => setVisible(document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', onChange);
    return () => document.removeEventListener('visibilitychange', onChange);
  }, []);
  return visible;
}

/** The PC refused this phone (no `watch`) or the pane is gone: asking again changes nothing. */
function refusalOf(error: unknown): string | null {
  const text = error instanceof Error ? error.message : String(error);
  if (/ -> 403/.test(text)) return NO_WATCH_NOTE;
  if (/ -> 404/.test(text)) return 'This agent is not running any more.';
  return null;
}

export function PhoneChatView({
  bridge,
  paneId,
  canWatch,
  canType,
  sendBlock = null,
  dock,
}: {
  bridge: BridgeClient | null;
  paneId: string;
  /** This phone may read pane text (`watch`). Without it nothing is asked. */
  canWatch: boolean;
  /** This phone may send messages to the agent (`type`). Without it the box is replaced by one line. */
  canType: boolean;
  /** Why the message box is switched off (shown instead of a hint), or null. */
  sendBlock?: string | null;
  /** What waits on the person (the docked card), under the conversation. */
  dock?: ReactNode;
}) {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [status, setStatus] = useState<Status>(LOADING);
  const visible = usePageVisible();
  /** The newest revision seen: the next question is "what changed since this". */
  const revRef = useRef<number | undefined>(undefined);
  const paneRef = useRef(paneId);
  if (paneRef.current !== paneId) {
    paneRef.current = paneId;
    revRef.current = undefined;
  }

  // A different pane starts from nothing.
  useEffect(() => {
    setItems([]);
    setStatus(LOADING);
  }, [paneId]);

  useEffect(() => {
    if (!bridge?.paneChat || !canWatch || !visible) return;
    let live = true;
    let timer = 0;
    const ask = bridge.paneChat.bind(bridge);

    const show = (next: Status) => setStatus((prev) => (sameStatus(prev, next) ? prev : next));

    const tick = async () => {
      let again = true;
      try {
        const reply = await ask(paneId, revRef.current);
        if (!live) return;
        if (!reply.available) {
          revRef.current = undefined;
          setItems((prev) => (prev.length === 0 ? prev : []));
          show({ state: 'unavailable', reason: reply.reason, waitingFor: null, source: null });
        } else {
          revRef.current = reply.rev;
          if (reply.reset || reply.items.length > 0) {
            setItems((prev) => mergeItems(prev, reply.items, reply.reset).slice(-MAX_KEPT));
          }
          show({ state: 'ready', reason: null, waitingFor: reply.waitingFor ?? null, source: reply.source ?? 'file' });
        }
      } catch (error) {
        if (!live) return;
        const refused = refusalOf(error);
        if (refused) {
          again = false;
          show({ state: 'unavailable', reason: refused, waitingFor: null, source: null });
        } else if (revRef.current === undefined) {
          // Never loaded: say so. Once there is a chat on screen a dropped answer is just tried again.
          show({ state: 'unavailable', reason: UNREACHABLE, waitingFor: null, source: null });
        }
      }
      if (live && again) timer = window.setTimeout(() => void tick(), POLL_MS);
    };
    void tick();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [bridge, paneId, canWatch, visible]);

  const grouped = useMemo(() => groupTurns(items), [items]);

  // The message box: its messages come over the bridge, and it needs to read the list (`watch`) as well as send.
  const canSend = canWatch && canType;
  const outboxTransport = useMemo(() => (bridge ? bridgeOutboxTransport(bridge) : null), [bridge]);
  const outbox = usePaneOutbox(outboxTransport, paneId, canSend);
  const userTexts = useMemo(() => grouped.turns.flatMap((turn) => (turn.user ? [turn.user.text] : [])), [grouped.turns]);
  const { bubbles, hide } = usePendingBubbles(outbox.messages, userTexts);
  const pending = canSend ? <PendingMessages messages={bubbles} onCancel={outbox.cancel} onHide={hide} /> : undefined;
  const composer = canSend ? (
    <ChatComposer onSend={outbox.send} blockedReason={sendBlock} voice draftKey={paneId} />
  ) : (
    <p className="ph-chat-send-off" data-testid="phone-chat-no-send">{NO_SEND_NOTE}</p>
  );

  if (!canWatch) {
    return (
      <div className="ph-chat" data-testid="phone-chat">
        <p className="ph-chat-plain">{NO_WATCH_NOTE}</p>
        {dock}
        {!canType && <p className="ph-chat-send-off" data-testid="phone-chat-no-send">{NO_SEND_NOTE}</p>}
      </div>
    );
  }
  return (
    <div className="ph-chat" data-testid="phone-chat">
      <SessionChatView
        state={status.state}
        reason={status.reason}
        waitingFor={status.waitingFor}
        source={status.source}
        turns={grouped.turns}
        model={grouped.model}
        cliVersion={grouped.cliVersion}
        paneId={paneId}
        dock={dock}
        pending={pending}
        composer={composer}
      />
    </div>
  );
}
