// The side question box over the chat: "Ask about this session". The answer comes from a one-shot read-only helper
// (lib/sideQuestion), shown here as Markdown with Copy and Ask again. Nothing is written into the pane and nothing is
// saved: closing the box forgets it. The box does not know any agent by name; the backend says who answers.
import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react';
import type { ChatTurn } from '../../lib/sessionChatModel';
import { buildSideContext, type SideQuestionApi, type SideTarget } from '../../lib/sideQuestion';
import { DotSpinner } from '../cockpit/DotSpinner';
import { ChatMarkdown } from './ChatMarkdown';

export const NO_SIDE_AGENT = 'No agent on this computer can answer a side question. Install one that can run without a pane, then try again.';
const MAX_QUESTION = 2_000;

type Phase =
  | { kind: 'idle' }
  | { kind: 'asking'; question: string }
  | { kind: 'answer'; question: string; text: string }
  | { kind: 'error'; question: string; message: string };

let nextAsk = 0;
const newAskId = (): string => `side-${Date.now().toString(36)}-${(nextAsk++).toString(36)}`;

export interface SideQuestionBoxProps {
  paneId: string;
  api: SideQuestionApi;
  /** The chat's turns now; read when the question is sent, so Ask again sees what happened since. */
  turns: readonly ChatTurn[];
  onClose: () => void;
}

export function SideQuestionBox({ paneId, api, turns, onClose }: SideQuestionBoxProps) {
  const [target, setTarget] = useState<SideTarget | null | undefined>(undefined);
  const [text, setText] = useState('');
  const [phase, setPhase] = useState<Phase>({ kind: 'idle' });
  const [copied, setCopied] = useState(false);
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
      setPhase({ kind: 'asking', question: q });
      api
        .ask(paneId, { ask_id: askId, question: q.slice(0, MAX_QUESTION), context: buildSideContext(turnsRef.current) })
        .then((result) => {
          if (current.current !== askId || !alive.current) return;
          if (result.ok && result.answer) setPhase({ kind: 'answer', question: q, text: result.answer });
          else setPhase({ kind: 'error', question: q, message: result.error?.message ?? 'No answer came back.' });
        })
        .catch((error: unknown) => {
          if (current.current !== askId || !alive.current) return;
          setPhase({ kind: 'error', question: q, message: error instanceof Error ? error.message : String(error) });
        });
    },
    [api, paneId, target],
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
  return (
    <section className="ck-side" role="dialog" aria-label="Ask about this session" data-testid="side-question">
      <header className="ck-side-head">
        <h2 className="ck-side-title">Ask about this session</h2>
        <button type="button" className="ck-side-close" aria-label="Close side question" onClick={onClose}>
          Esc
        </button>
      </header>
      <p className="ck-side-note">
        {unavailable ? NO_SIDE_AGENT : 'A helper reads the recent chat and answers here. Your agent does not see this.'}
      </p>
      <form
        className="ck-side-row"
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
          className="ck-side-field"
          onChange={(event) => setText(event.target.value)}
          onKeyDown={onFieldKeyDown}
        />
        <button type="submit" className="ck-btn ck-btn--primary" disabled={!target || asking || text.trim() === ''}>
          Ask
        </button>
      </form>
      {asking && (
        <p className="ck-side-status" role="status" data-testid="side-question-loading">
          <DotSpinner status="running" size={3} label="Asking" />
          <span>{target ? `Asking ${target.label}…` : 'Asking…'}</span>
        </p>
      )}
      {phase.kind === 'error' && (
        <p className="ck-side-error" role="alert" data-testid="side-question-error">
          {phase.message}
        </p>
      )}
      {phase.kind === 'answer' && (
        <div className="ck-side-answer" data-testid="side-question-answer">
          <div className="ck-side-answer-body">
            <ChatMarkdown text={phase.text} />
          </div>
          <div className="ck-side-actions">
            <button type="button" className="ck-btn" onClick={() => void copy()}>
              {copied ? 'Copied' : 'Copy'}
            </button>
            <button type="button" className="ck-btn" onClick={() => ask(phase.question)}>
              Ask again
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
