// Read-only chat of a terminal pane's agent session: the person's messages, the agent's text, and each turn's tool
// calls folded into one activity line (click to open the rows and their detail). A pinned task card sits at the top
// when the agent has a plan, a thin line marks a notice, and a turn ends with how long it worked. The view mode
// (Summary, Normal, Verbose) is in the header. The agent's text is Markdown when it comes from the session file;
// text read off the terminal is shown as it is.
import { useLayoutEffect, useMemo, useRef, type ReactNode } from 'react';
import type { ChatSource, ChatTurn, SessionChatState, TurnStep } from '../../lib/sessionChatModel';
import '../../styles/cockpit-chat.css';
import { ActivityLine } from './ActivityLine';
import { segmentSteps } from './chatActivity';
import { formatDuration } from './chatFormat';
import { useChatViewMode, type ChatViewMode } from './chatViewMode';
import { ChatMarkdown } from './ChatMarkdown';
import { HelperRow } from './HelperRow';
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
  /** Messages the person wrote that the conversation does not show yet: bubbles at the end of the conversation. */
  pending?: ReactNode;
  /** The message box, under the dock. Without it the chat is read-only. */
  composer?: ReactNode;
  /** The pane the chat belongs to: its view mode is remembered under it. Without one the mode is Normal and not kept. */
  paneId?: string | null;
  /** Offer "Remember for all agents" on selected text (desktop Chat tab only). Needs `paneId`. */
  remember?: boolean;
}

const SUMMARY_NOTE = 'Summary: your messages, final answers and activity lines.';

function TextStep({ step, markdown, mode }: { step: Extract<TurnStep, { kind: 'text' }>; markdown: boolean; mode: ChatViewMode }) {
  // Summary keeps only the agent's final answers; the rest of its talk is folded away with the rows.
  if (mode === 'summary' && !step.item.finalAnswer) return null;
  if (markdown) return <ChatMarkdown text={step.item.text} />;
  return <div className="whitespace-pre-wrap break-words ck-chat-text">{step.item.text}</div>;
}

function TurnEndDivider({ turn }: { turn: ChatTurn }) {
  const parts: string[] = [];
  if (turn.outcome === 'interrupted' || turn.outcome === 'failed') parts.push(turn.outcome);
  if (turn.durationMs !== null) parts.push(`Worked for ${formatDuration(turn.durationMs)}`);
  return (
    <div className="flex items-center gap-2 py-0.5 text-[10px] text-pm-muted ck-chat-end">
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
      {parts.length > 0 && <span className="ck-chat-end-text">{parts.join(' · ')}</span>}
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
    </div>
  );
}

function TurnView({ turn, fromScreen, markdown, mode }: { turn: ChatTurn; fromScreen: boolean; markdown: boolean; mode: ChatViewMode }) {
  const verbose = mode === 'verbose';
  // A turn with no end yet is still running: its rows without a result are live.
  const live = turn.end === null;
  return (
    <section className="space-y-1.5 ck-chat-turn">
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
      {turn.end && <TurnEndDivider turn={turn} />}
    </section>
  );
}

export function SessionChatView({ state, reason, waitingFor, source, turns, model, cliVersion, dock, pending, composer, paneId, remember }: SessionChatViewProps) {
  const fromScreen = source === 'screen';
  // Markdown only for the agent's session file; an unknown source keeps the text as it is.
  const markdown = source === 'file';
  const [mode, setMode] = useChatViewMode(paneId);
  const plan = useMemo(() => currentPlan(turns), [turns]);
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

  const onScroll = () => {
    const el = scrollRef.current;
    if (el) atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight <= AT_BOTTOM_PX;
  };

  return (
    <div className="flex h-full min-h-0 flex-col bg-pm-bg text-xs text-pm-text ck-chat" data-testid="session-chat" data-view={mode}>
      <header className="flex min-w-0 flex-wrap items-center gap-2 border-b border-pm-border bg-pm-panel px-2 py-1 ck-chat-head">
        <span className="min-w-0 truncate ck-chat-model">{model ?? 'Model not known yet'}</span>
        {cliVersion && <span className="shrink-0 text-pm-muted ck-chat-cli">CLI {cliVersion}</span>}
        <ViewModeToggle mode={mode} onChange={setMode} />
      </header>
      <p className="border-b border-pm-border px-2 py-1 text-[11px] text-pm-muted ck-chat-note">
        {SESSION_CHAT_NOTE}
        {fromScreen && (
          <span className="block ck-chat-screen-note" data-testid="session-chat-screen-note">
            {SCREEN_CHAT_NOTE}
          </span>
        )}
        {mode === 'summary' && <span className="block ck-chat-mode-note">{SUMMARY_NOTE}</span>}
      </p>
      <PlanCard items={plan} />
      <div ref={scrollRef} onScroll={onScroll} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-2 py-2 ck-chat-body">
        {state === 'unavailable' && (
          <div className="ck-empty ck-chat-empty">
            <h2 className="ck-section text-xs font-semibold text-pm-text">Chat unavailable</h2>
            <p className="ck-sub text-pm-muted">{reason ?? 'Chat is not available for this pane.'}</p>
          </div>
        )}
        {state !== 'unavailable' && turns.length === 0 && (
          <div className="ck-chat-waiting">
            <p className="ck-sub text-pm-muted">
              {fromScreen ? 'Waiting for a message on the terminal…' : "Waiting for the agent's session file…"}
            </p>
            {waitingFor && (
              <p className="mt-0.5 break-words text-[11px] text-pm-muted opacity-70 ck-chat-waiting-for">{waitingFor}</p>
            )}
          </div>
        )}
        {state !== 'unavailable' &&
          turns.map((turn) => <TurnView key={`${mode}:${turn.id}`} turn={turn} fromScreen={fromScreen} markdown={markdown} mode={mode} />)}
        {state !== 'unavailable' && pending}
      </div>
      {remember && paneId && <RememberSelection paneId={paneId} root={scrollRef} />}
      {dock && <div className="ck-chat-dock">{dock}</div>}
      {state !== 'unavailable' && composer}
    </div>
  );
}
