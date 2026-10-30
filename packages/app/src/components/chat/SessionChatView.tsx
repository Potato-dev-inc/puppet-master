// Read-only chat of a terminal pane's agent session: the person's messages, the agent's text, and each turn's tool
// calls folded into one activity line (click to open the rows and their detail). A pinned task card sits at the top
// when the agent has a plan, a thin line marks a notice, and a turn ends with how long it worked. The view mode
// (Summary, Normal, Verbose) is in the header. The agent's text is Markdown when it comes from the session file;
// text read off the terminal is shown as it is.
import { useEffect, useLayoutEffect, useMemo, useRef, type ReactNode } from 'react';
import { clearChatJump, turnWithItem, type ChatJump } from '../../lib/chatJump';
import type { ChatSource, ChatTurn, SessionChatState, TurnStep } from '../../lib/sessionChatModel';
import '../../styles/cockpit-chat.css';
import { ActivityLine } from './ActivityLine';
import { segmentSteps } from './chatActivity';
import { formatDuration } from './chatFormat';
import { useChatViewMode, type ChatViewMode } from './chatViewMode';
import { ChatMarkdown } from './ChatMarkdown';
import { HelperRow } from './HelperRow';
import { PaneHelperRows } from './PaneHelperRows';
import { placeHelpers, type ChatHelper } from '../../lib/chat-helpers';
import { NoticeLine } from './NoticeLine';
import { RememberSelection } from './RememberSelection';
import { currentPlan, PlanCard } from './PlanCard';
import { ToolRow } from './ToolRow';
import { ViewModeToggle } from './ViewModeToggle';

export { formatDuration };

export const SESSION_CHAT_NOTE = 'Messages wait until the agent is free. Answer its questions in the card at the bottom.';
/** Under the note when the chat is read off the terminal's text: a reading, not the agent's own record. */
export const SCREEN_CHAT_NOTE = 'Read from the screen: text may be cut or wrapped.';

/** Within this many pixels of the end counts as "at the bottom" (auto-scroll follows only then). */
const AT_BOTTOM_PX = 24;

export interface SessionChatViewProps {
  state: SessionChatState;
  reason: string | null;
  /** What an empty chat is waiting for, under the waiting line (where the reader looks). */
  waitingFor?: string | null;
  /** Where the items are read from. `screen`: the terminal's text (no tool output; text may be cut or wrapped). */
  source?: ChatSource | null;
  turns: ChatTurn[];
  model: string | null;
  cliVersion: string | null;
  /** What waits on the person (a question card, a bar): shown under the conversation, outside its scroll. */
  dock?: ReactNode;
  /** Another row under the dock, outside it (a card that brings its own dock when it has something to say). */
  aside?: ReactNode;
  /** Messages the person wrote that the conversation does not show yet: bubbles at the end of the conversation. */
  pending?: ReactNode;
  /** The message box, under the dock. Without it the chat is read-only. */
  composer?: ReactNode;
  /** The pane the chat belongs to: its view mode is remembered under it. Without one the mode is Normal and not kept. */
  paneId?: string | null;
  /** Offer "Remember for all agents" on selected text (desktop Chat tab only). Needs `paneId`. */
  remember?: boolean;
  /** Shown at the top of the chat while it has no turns yet (a companion's "new session" line). */
  emptyNote?: ReactNode;
  /** A control next to the view mode switch in the header (the side question button). */
  headerExtra?: ReactNode;
  /** The chat menu at the end of the header (Save as page). */
  headerMenu?: ReactNode;
  /** A one-line note above the conversation (the Keep going line); it shows nothing when there is nothing to say. */
  banner?: ReactNode;
  /** Something laid over the chat, inside it (the side question box). */
  overlay?: ReactNode;
  /** A search result asked for this message: the chat scrolls to it once it is there. */
  jump?: ChatJump | null;
  /** Show `Make clip` on finished turns (desktop Chat tab only). Needs `onMakeClip`. */
  canClip?: boolean;
  /** `Make clip` was pressed on the turn that the message with this id starts. */
  onMakeClip?: (userItemId: string) => void;
  /** An old chat with no live pane behind it: the "Messages wait…" note would be wrong there, so it is left out. */
  readOnly?: boolean;
  /** Show `Branch` on finished turns (desktop Chat tab only). Needs `onBranch`. Never shown when `readOnly`. */
  canBranch?: boolean;
  /** `Branch` was pressed on the turn that the message with this id starts. */
  onBranch?: (userItemId: string) => void;
  /** The helpers this pane's agent started through Puppet Master: compact rows in the turn they were started in. */
  helpers?: readonly ChatHelper[];
  /** A helper row was pressed: open that pane. */
  onOpenPane?: (paneId: string) => void;
}

/** The button on a finished turn's end line. `always`: shown without hover (the newest finished turn). */
interface TurnClip {
  onMake: () => void;
  always: boolean;
}

/** The `Branch` button on a finished turn's end line. Same rules as `TurnClip`. */
interface TurnBranch {
  onBranch: () => void;
  always: boolean;
}

/** A finished turn that a message started: the only kind a clip can be made of. */
export function isClippable(turn: ChatTurn): turn is ChatTurn & { user: NonNullable<ChatTurn['user']> } {
  return turn.end !== null && turn.user !== null;
}

/** The id of the newest clippable turn, or null. */
export function lastClippableId(turns: readonly ChatTurn[]): string | null {
  for (let i = turns.length - 1; i >= 0; i--) if (isClippable(turns[i])) return turns[i].id;
  return null;
}

/** How long a jump waits for its message to load before it is dropped. */
const JUMP_WAIT_MS = 10_000;

const SUMMARY_NOTE = 'Summary: your messages, final answers and activity lines.';

function TextStep({ step, markdown, mode }: { step: Extract<TurnStep, { kind: 'text' }>; markdown: boolean; mode: ChatViewMode }) {
  // Summary keeps only the agent's final answers; the rest of its talk is folded away with the rows.
  if (mode === 'summary' && !step.item.finalAnswer) return null;
  if (markdown) return <ChatMarkdown text={step.item.text} />;
  return <div className="whitespace-pre-wrap break-words ck-chat-text">{step.item.text}</div>;
}

function TurnEndDivider({ turn, clip, branch }: { turn: ChatTurn; clip?: TurnClip; branch?: TurnBranch }) {
  const parts: string[] = [];
  if (turn.outcome === 'interrupted' || turn.outcome === 'failed') parts.push(turn.outcome);
  if (turn.durationMs !== null) parts.push(`Worked for ${formatDuration(turn.durationMs)}`);
  return (
    <div className="flex items-center gap-2 py-0.5 text-[10px] text-pm-muted ck-chat-end">
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
      {parts.length > 0 && <span className="ck-chat-end-text">{parts.join(' · ')}</span>}
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
      {branch && (
        <button type="button" className={`ck-chat-branch-btn${branch.always ? ' is-always' : ''}`} onClick={branch.onBranch}>
          Branch
        </button>
      )}
      {clip && (
        <button type="button" className={`ck-chat-clip-btn${clip.always ? ' is-always' : ''}`} onClick={clip.onMake}>
          Make clip
        </button>
      )}
    </div>
  );
}

function TurnView({ turn, fromScreen, markdown, mode, clip, branch, helpers, onOpenPane }: { turn: ChatTurn; fromScreen: boolean; markdown: boolean; mode: ChatViewMode; clip?: TurnClip; branch?: TurnBranch; helpers?: readonly ChatHelper[]; onOpenPane?: (paneId: string) => void }) {
  const verbose = mode === 'verbose';
  // A turn with no end yet is still running: its rows without a result are live.
  const live = turn.end === null;
  return (
    <section className="space-y-1.5 ck-chat-turn" data-turn-id={turn.id}>
      {turn.user && (
        <div className="flex justify-end">
          <div className="max-w-[85%] whitespace-pre-wrap break-words rounded bg-pm-raised px-2 py-1 text-pm-text ck-chat-bubble">
            {turn.user.text}
          </div>
        </div>
      )}
      {segmentSteps(turn.steps).map((segment) => {
        if (segment.kind === 'activity') {
          return <ActivityLine key={segment.id} steps={segment.steps} live={live} fromScreen={fromScreen} verbose={verbose} />;
        }
        const step = segment.step;
        if (step.kind === 'tool') {
          return step.call.helper ? (
            <HelperRow key={step.id} step={step} fromScreen={fromScreen} defaultOpen={verbose} />
          ) : (
            <ToolRow key={step.id} step={step} fromScreen={fromScreen} defaultOpen={verbose} />
          );
        }
        if (step.kind === 'notice') {
          return mode === 'summary' ? null : <NoticeLine key={step.id} text={step.item.text} kind={step.item.noticeKind} url={step.item.url} />;
        }
        return <TextStep key={step.id} step={step} markdown={markdown} mode={mode} />;
      })}
      {helpers && helpers.length > 0 && <PaneHelperRows helpers={helpers} onOpen={onOpenPane} />}
      {turn.end && <TurnEndDivider turn={turn} clip={clip} branch={branch} />}
    </section>
  );
}

export function SessionChatView({ state, reason, waitingFor, source, turns, model, cliVersion, dock, pending, composer, paneId, remember, emptyNote, headerExtra, headerMenu, banner, overlay, aside, jump, readOnly, canClip, onMakeClip, canBranch, onBranch, helpers, onOpenPane }: SessionChatViewProps) {
  const fromScreen = source === 'screen';
  // Markdown only for what the agent itself wrote (its session file or its API); an unknown source keeps the text as it is.
  const markdown = source === 'file' || source === 'api';
  const [mode, setMode] = useChatViewMode(paneId);
  const plan = useMemo(() => currentPlan(turns), [turns]);
  // The newest finished turn that a message started keeps its `Make clip` button in view; the others show it on hover.
  const newestClippable = useMemo(() => (canClip && onMakeClip ? lastClippableId(turns) : null), [canClip, onMakeClip, turns]);
  // The same rule for `Branch`.
  const showBranch = !!canBranch && !!onBranch && !readOnly;
  const newestBranchable = useMemo(() => (showBranch ? lastClippableId(turns) : null), [showBranch, turns]);
  const placedHelpers = useMemo(() => placeHelpers(turns, helpers ?? []), [turns, helpers]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);

  // Follow new items only while the person is already at the bottom.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [turns, pending]);

  // A docked card takes height from the conversation when it appears or goes: a reader at the end stays at the
  // end; a reader scrolled up keeps the same place.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      if (atBottom.current) el.scrollTop = el.scrollHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Scroll to the message a search result opened and wash it for a moment. A message that never comes (the chat
  // does not hold it any more) is dropped after a while, and so is one that is not in a chat that has loaded.
  useEffect(() => {
    if (!jump) return;
    const turn = turnWithItem(turns, jump.itemId);
    const el = turn ? [...(scrollRef.current?.querySelectorAll<HTMLElement>('[data-turn-id]') ?? [])].find((node) => node.dataset.turnId === turn.id) : undefined;
    if (el) {
      atBottom.current = false;
      el.scrollIntoView({ block: 'center' });
      el.classList.add('is-found');
      setTimeout(() => el.classList.remove('is-found'), 2000);
      clearChatJump(jump);
      return;
    }
    if (state === 'unavailable') {
      clearChatJump(jump);
      return;
    }
    const giveUp = setTimeout(() => clearChatJump(jump), JUMP_WAIT_MS);
    return () => clearTimeout(giveUp);
  }, [jump, turns, state]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (el) atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= AT_BOTTOM_PX;
  };

  return (
    <div className="relative flex h-full min-h-0 flex-col bg-pm-bg text-xs text-pm-text ck-chat" data-testid="session-chat" data-view={mode}>
      <header className="flex min-w-0 flex-wrap items-center gap-2 border-b border-pm-border bg-pm-panel px-2 py-1 ck-chat-head">
        <span className="min-w-0 truncate ck-chat-model">{model ?? 'Model not known yet'}</span>
        {cliVersion && <span className="shrink-0 text-pm-muted ck-chat-cli">CLI {cliVersion}</span>}
        {headerExtra && <span className="ml-auto flex shrink-0 items-center ck-chat-head-extra">{headerExtra}</span>}
        <ViewModeToggle mode={mode} onChange={setMode} />
        {headerMenu && <span className="flex shrink-0 items-center ck-chat-head-menu">{headerMenu}</span>}
      </header>
      {(!readOnly || fromScreen || mode === 'summary') && (
        <p className="border-b border-pm-border px-2 py-1 text-[11px] text-pm-muted ck-chat-note">
          {!readOnly && SESSION_CHAT_NOTE}
          {fromScreen && (
            <span className="block ck-chat-screen-note" data-testid="session-chat-screen-note">
              {SCREEN_CHAT_NOTE}
            </span>
          )}
          {mode === 'summary' && <span className="block ck-chat-mode-note">{SUMMARY_NOTE}</span>}
        </p>
      )}
      {banner}
      <PlanCard items={plan} />
      <div ref={scrollRef} onScroll={onScroll} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-2 py-2 ck-chat-body">
        {state === 'unavailable' && (
          <div className="ck-empty ck-chat-empty">
            <h2 className="ck-section text-xs font-semibold text-pm-text">Chat unavailable</h2>
            <p className="ck-sub text-pm-muted">{reason ?? 'Chat is not available for this pane.'}</p>
          </div>
        )}
        {state !== 'unavailable' && turns.length === 0 && emptyNote}
        {state !== 'unavailable' && turns.length === 0 && (
          <div className="ck-chat-waiting">
            <p className="ck-sub text-pm-muted">
              {fromScreen ? 'Waiting for a message on the terminal…' : source === 'api' ? "Waiting for the agent's session…" : "Waiting for the agent's session file…"}
            </p>
            {waitingFor && (
              <p className="mt-0.5 break-words text-[11px] text-pm-muted opacity-70 ck-chat-waiting-for">{waitingFor}</p>
            )}
          </div>
        )}
        {state !== 'unavailable' &&
          turns.map((turn) => (
            <TurnView
              key={`${mode}:${turn.id}`}
              turn={turn}
              fromScreen={fromScreen}
              markdown={markdown}
              mode={mode}
              clip={canClip && onMakeClip && isClippable(turn) ? { onMake: () => onMakeClip(turn.user.id), always: turn.id === newestClippable } : undefined}
              branch={canBranch && !readOnly && onBranch && isClippable(turn) ? { onBranch: () => onBranch(turn.user.id), always: turn.id === newestBranchable } : undefined}
              helpers={placedHelpers.get(turn.id)}
              onOpenPane={onOpenPane}
            />
          ))}
        {state !== 'unavailable' && pending}
      </div>
      {remember && paneId && <RememberSelection paneId={paneId} root={scrollRef} />}
      {dock && <div className="ck-chat-dock">{dock}</div>}
      {aside}
      {state !== 'unavailable' && composer}
      {overlay}
    </div>
  );
}
