// The side question box over the chat: "Ask about this session". The answer comes from a one-shot read-only helper
// (lib/sideQuestion), shown here as Markdown with Copy, Ask again and Remember. Nothing is written into the pane and
// nothing is saved to disk: closing the box keeps only the pane's last few answers in memory (useSideQuestion), listed
// under "Earlier". The box does not know any agent by name; the backend says who answers.
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react';
import { rememberApi, rememberErrorMessage, type RememberApi } from '../../lib/chat-remember';
import type { ChatTurn } from '../../lib/sessionChatModel';
import { buildSideContext, type SideQuestionApi, type SideTarget } from '../../lib/sideQuestion';
import { DotSpinner } from '../cockpit/DotSpinner';
import { ChatMarkdown } from './ChatMarkdown';
import { REMEMBER_ALREADY, REMEMBER_REMOVED, REMEMBER_SAVED } from './RememberSelection';
import type { SideAnswer } from './useSideQuestion';

export const NO_SIDE_AGENT = 'No agent on this computer can answer a side question. Install one that can run without a pane, then try again.';
const MAX_QUESTION = 2_000;
const NO_RECENT: readonly SideAnswer[] = [];

type Phase =
  | { kind: 'idle' }
  | { kind: 'asking'; question: string }
  | { kind: 'answer'; id: string; question: string; text: string }
  | { kind: 'error'; question: string; message: string };

/** The line under the answer after Remember: what happened, and Undo when a new line was stored. */
interface Saved {
  message: string;
  /** The stored line Undo removes. */
  undo?: string;
}

let nextAsk = 0;
const newAskId = (): string => `side-${Date.now().toString(36)}-${(nextAsk++).toString(36)}`;

export interface SideQuestionBoxProps {
  paneId: string;
  api: SideQuestionApi;
  /** The chat's turns now; read when the question is sent, so Ask again sees what happened since. */
  turns: readonly ChatTurn[];
  /** The pane's last answers, newest first (kept by useSideQuestion). */
  recent?: readonly SideAnswer[];
  /** Called once for each answer shown, so the pane keeps it. */
  onAnswer?: (entry: SideAnswer) => void;
  /** The "Remember for all agents" API; the same one the chat selection uses. */
  memory?: RememberApi;
  onClose: () => void;
}

export function SideQuestionBox({ paneId, api, turns, recent = NO_RECENT, onAnswer, memory = rememberApi, onClose }: SideQuestionBoxProps) {
  const [target, setTarget] = useState<SideTarget | null | undefined>(undefined);
  const [text, setText] = useState('');
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState<Saved | null>(null);
  const [openEarlier, setOpenEarlier] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const turnsRef = useRef(turns);
  turnsRef.current = turns;
  // Only the newest question may answer; closing the box drops a running one.
  const current = useRef<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    field.current?.focus();
    api
      .target(paneId)
      .then((t) => alive.current && setTarget(t))
      .catch(() => alive.current && setTarget(null));
    return () => {
      alive.current = false;
      current.current = null;
    };
  }, [api, paneId]);

  const ask = useCallback(
    (question: string) => {
      const q = question.trim();
      if (q === '' || !target) return;
      const askId = newAskId();
      current.current = askId;
      setCopied(false);
      setSaved(null);
      setPhase({ kind: 'asking', question: q });
      api
        .ask(paneId, { ask_id: askId, question: q.slice(0, MAX_QUESTION), context: buildSideContext(turnsRef.current) })
        .then((result) => {
          if (current.current !== askId || !alive.current) return;
          if (result.ok && result.answer) {
            setPhase({ kind: 'answer', id: askId, question: q, text: result.answer });
            onAnswer?.({ id: askId, question: q, answer: result.answer });
          } else setPhase({ kind: 'error', question: q, message: result.error?.message ?? 'No answer came back.' });
        })
        .catch((error: unknown) => {
          if (current.current !== askId || !alive.current) return;
          setPhase({ kind: 'error', question: q, message: error instanceof Error ? error.message : String(error) });
        });
    },
    [api, paneId, target, onAnswer],
  );

  const copy = async () => {
    if (phase.kind !== 'answer') return;
    try {
      await navigator.clipboard.writeText(phase.text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  // Remember: the answer becomes one line of the project memory. Undo removes that line again.
  const rememberAnswer = async (line: string) => {
    setSaved(null);
    try {
      const done = await memory.remember(paneId, line);
      setSaved(done.added ? { message: REMEMBER_SAVED, undo: done.text } : { message: REMEMBER_ALREADY });
    } catch (error) {
      setSaved({ message: rememberErrorMessage(error) });
    }
  };
  const undoRemember = async (line: string) => {
    try {
      await memory.forget(paneId, line);
      setSaved({ message: REMEMBER_REMOVED });
    } catch (error) {
      setSaved({ message: rememberErrorMessage(error) });
    }
  };

  // Esc closes the box wherever the keyboard is while it is open.
  useEffect(() => {
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  const onFieldKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    if (phase.kind !== 'asking') ask(text);
  };

  const asking = phase.kind === 'asking';
  const unavailable = target === null;
  // The answer on screen is not repeated under "Earlier".
  const earlier = recent.filter((entry) => phase.kind !== 'answer' || entry.id !== phase.id);
  return (
    <section className="ck-sq" role="dialog" aria-label="Ask about this session" data-testid="side-question">
      <header className="ck-sq-head">
        <h2 className="ck-sq-title">Ask about this session</h2>
        <button type="button" className="ck-sq-close" aria-label="Close side question" onClick={onClose}>
          Esc
        </button>
      </header>
      <p className="ck-sq-note">
        {unavailable ? NO_SIDE_AGENT : 'A helper reads the recent chat and answers here. Your agent does not see this.'}
      </p>
      <form
        className="ck-sq-row"
        onSubmit={(event) => {
          event.preventDefault();
          if (!asking) ask(text);
        }}
      >
        <textarea
          ref={field}
          rows={1}
          value={text}
          maxLength={MAX_QUESTION}
          disabled={unavailable}
          aria-label="Your question"
          placeholder="What do you want to know?"
          className="ck-sq-field"
          onChange={(event) => setText(event.target.value)}
          onKeyDown={onFieldKeyDown}
        />
        <button type="submit" className="ck-btn ck-btn--primary" disabled={!target || asking || text.trim() === ''}>
          Ask
        </button>
      </form>
      {asking && (
        <p className="ck-sq-status" role="status" data-testid="side-question-loading">
          <DotSpinner status="running" size={3} label="Asking" />
          <span>{target ? `Asking ${target.label}…` : 'Asking…'}</span>
        </p>
      )}
      {phase.kind === 'error' && (
        <p className="ck-sq-error" role="alert" data-testid="side-question-error">
          {phase.message}
        </p>
      )}
      {phase.kind === 'answer' && (
        <div className="ck-sq-answer" data-testid="side-question-answer">
          <div className="ck-sq-answer-body">
            <ChatMarkdown text={phase.text} />
          </div>
          <div className="ck-sq-actions">
            <button type="button" className="ck-btn" onClick={() => void copy()}>
              {copied ? 'Copied' : 'Copy'}
            </button>
            <button type="button" className="ck-btn" onClick={() => ask(phase.question)}>
              Ask again
            </button>
            <button type="button" className="ck-btn" title="Remember for all agents" onClick={() => void rememberAnswer(phase.text)}>
              Remember
            </button>
          </div>
          {saved && (
            <p className="ck-sq-status" role="status" data-testid="side-question-remember">
              <span>{saved.message}</span>
              {saved.undo !== undefined && (
                <button type="button" className="ck-btn" onClick={() => void undoRemember(saved.undo ?? '')}>
                  Undo
                </button>
              )}
            </p>
          )}
        </div>
      )}
      {earlier.length > 0 && (
        <div className="ck-sq-earlier" data-testid="side-question-earlier">
          <h3 className="ck-sq-earlier-title">Earlier</h3>
          <ul className="ck-sq-earlier-list">
            {earlier.map((entry) => {
              const expanded = openEarlier === entry.id;
              return (
                <li key={entry.id}>
                  <button type="button" className="ck-sq-earlier-q" aria-expanded={expanded} onClick={() => setOpenEarlier(expanded ? null : entry.id)}>
                    {entry.question}
                  </button>
                  {expanded && (
                    <div className="ck-sq-answer-body" data-testid="side-question-earlier-answer">
                      <ChatMarkdown text={entry.answer} />
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </section>
  );
}
