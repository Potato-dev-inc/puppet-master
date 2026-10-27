// Read-only chat of a terminal pane's agent session: the person's messages, the agent's text, each tool
// call as one row (click to see its output) and a divider where a turn ended. Text is plain (no markdown).
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { ChatSource, ChatTurn, SessionChatState, ToolStep, TurnStep } from '../../lib/sessionChatModel';
import '../../styles/cockpit-chat.css';

export const SESSION_CHAT_NOTE = "Read-only. You can answer the agent's questions in the card at the bottom.";
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
}

/** "800ms" is "<1s"; "12s"; "2m 5s". */
export function formatDuration(ms: number): string {
  if (ms < 1000) return '<1s';
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

const STATUS_LABEL: Record<ToolStep['status'], string> = {
  running: 'running',
  ok: 'ok',
  error: 'error',
};

function ToolRow({ step, fromScreen }: { step: ToolStep; fromScreen: boolean }) {
  const [open, setOpen] = useState(false);
  const { call, result, status } = step;
  // A tool row read off the screen is just the row: no name, no result and no status to report.
  const label = fromScreen ? null : STATUS_LABEL[status];
  return (
    <div>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="flex w-full min-w-0 items-center gap-2 rounded px-1.5 py-0.5 text-left hover:bg-pm-raised ck-chat-tool"
      >
        {call.name && <span className="shrink-0 font-mono text-pm-text ck-chat-tool-name">{call.name}</span>}
        <span className="min-w-0 flex-1 truncate font-mono text-pm-muted ck-chat-tool-summary" title={call.summary}>
          {call.summary}
        </span>
        {label && (
          <span className={`shrink-0 text-[10px] ${status === 'error' ? 'text-pm-err' : 'text-pm-muted'} ck-chat-tool-status${status === 'error' ? ' ck-chat-tool-status--error' : ''}`}>
            {label}
          </span>
        )}
      </button>
      {open && (
        <pre className="mt-0.5 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-pm-border bg-pm-panel p-1.5 font-mono text-[11px] text-pm-muted ck-chat-output">
          {result ? result.output : fromScreen ? 'Tool output is not read from the screen.' : 'No output yet.'}
        </pre>
      )}
    </div>
  );
}

function Step({ step, fromScreen }: { step: TurnStep; fromScreen: boolean }) {
  if (step.kind === 'tool') return <ToolRow step={step} fromScreen={fromScreen} />;
  return <div className="whitespace-pre-wrap break-words ck-chat-text">{step.item.text}</div>;
}

function TurnEndDivider({ turn }: { turn: ChatTurn }) {
  const parts: string[] = [];
  if (turn.outcome === 'interrupted' || turn.outcome === 'failed') parts.push(turn.outcome);
  if (turn.durationMs !== null) parts.push(formatDuration(turn.durationMs));
  return (
    <div className="flex items-center gap-2 py-0.5 text-[10px] text-pm-muted ck-chat-end">
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
      {parts.length > 0 && <span>{parts.join(' · ')}</span>}
      <span className="h-px flex-1 bg-pm-border ck-chat-rule" />
    </div>
  );
}

function TurnView({ turn, fromScreen }: { turn: ChatTurn; fromScreen: boolean }) {
  return (
    <section className="space-y-1.5 ck-chat-turn">
      {turn.user && (
        <div className="flex justify-end">
          <div className="max-w-[85%] whitespace-pre-wrap break-words rounded bg-pm-raised px-2 py-1 text-pm-text ck-chat-bubble">
            {turn.user.text}
          </div>
        </div>
      )}
      {turn.steps.map((step) => (
        <Step key={step.id} step={step} fromScreen={fromScreen} />
      ))}
      {turn.end && <TurnEndDivider turn={turn} />}
    </section>
  );
}

export function SessionChatView({ state, reason, waitingFor, source, turns, model, cliVersion, dock }: SessionChatViewProps) {
  const fromScreen = source === 'screen';
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);

  // Follow new items only while the person is already at the bottom.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && atBottom.current) el.scrollTop = el.scrollHeight;
  }, [turns]);

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
    <div className="flex h-full min-h-0 flex-col bg-pm-bg text-xs text-pm-text ck-chat" data-testid="session-chat">
      <header className="flex min-w-0 items-baseline gap-2 border-b border-pm-border bg-pm-panel px-2 py-1 ck-chat-head">
        <span className="min-w-0 truncate ck-chat-model">{model ?? 'Model not known yet'}</span>
        {cliVersion && <span className="shrink-0 text-pm-muted ck-chat-cli">CLI {cliVersion}</span>}
      </header>
      <p className="border-b border-pm-border px-2 py-1 text-[11px] text-pm-muted ck-chat-note">
        {SESSION_CHAT_NOTE}
        {fromScreen && (
          <span className="block ck-chat-screen-note" data-testid="session-chat-screen-note">
            {SCREEN_CHAT_NOTE}
          </span>
        )}
      </p>
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
        {state !== 'unavailable' && turns.map((turn) => <TurnView key={turn.id} turn={turn} fromScreen={fromScreen} />)}
      </div>
      {dock && <div className="ck-chat-dock">{dock}</div>}
    </div>
  );
}
