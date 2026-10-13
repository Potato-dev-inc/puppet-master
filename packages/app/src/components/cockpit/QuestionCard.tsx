// One `ask_user` question in the Inbox: the agent's question, its choices as buttons, a text field
// when free text is allowed, and Dismiss. Rendered like a prompt card, marked "Question from <agent>".
import { useState } from 'react';
import { askerName, waitCaption, type Question, type QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { DotSpinner } from './DotSpinner';
import { formatElapsed } from './model';

export function QuestionCard({
  q,
  paneName,
  now,
  onAnswer,
  onDismiss,
  onOpenPane,
}: {
  q: Question;
  /** Name of the pane the question belongs to, when it names one. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onDismiss: () => void;
  onOpenPane?: () => void;
}) {
  const [text, setText] = useState('');
  const send = () => {
    const value = text.trim();
    if (!value) return;
    onAnswer({ text: value });
    setText('');
  };

  return (
    <li className={`ck-inbox-item${q.urgency === 'high' ? ' is-urgent' : ''}`} data-question-id={q.id}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status="waiting_input" />
        </span>
        <span className="ck-row-title">Question from {askerName(q, paneName)}</span>
        <span className="ck-row-time">{`asked ${formatElapsed(now - q.created_at_ms)} ago`}</span>
      </div>
      <p className="ck-q-text">{q.question}</p>
      {q.context && <p className="ck-q-ctx">{q.context}</p>}
      {q.choices.length > 0 && (
        <div className="ck-inbox-actions">
          {/* Equal weight on purpose: the agent wrote the choices, so none is made the easy default. */}
          {q.choices.map((c) => (
            <button
              key={c.id}
              type="button"
              className="ck-btn ck-btn--secondary"
              onClick={() => onAnswer({ choice_id: c.id })}
            >
              {c.label}
            </button>
          ))}
        </div>
      )}
      <p className="ck-q-wait" data-testid="question-wait">
        {waitCaption(q, now, paneName)}
      </p>
      {q.allow_free_text && (
        <form
          className="ck-q-field"
          onSubmit={(e) => {
            e.preventDefault();
            send();
          }}
        >
          <input
            className="ck-q-input"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={q.choices.length > 0 ? 'Or type an answer' : 'Type an answer'}
            aria-label="Your answer"
            maxLength={4000}
          />
          <button type="submit" className="ck-btn ck-btn--secondary" disabled={!text.trim()}>
            Send
          </button>
        </form>
      )}
      <div className="ck-inbox-actions">
        <button type="button" className="ck-btn ck-btn--plain" onClick={onDismiss}>
          Dismiss
        </button>
        {onOpenPane && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenPane}>
            Open terminal
          </button>
        )}
      </div>
    </li>
  );
}
