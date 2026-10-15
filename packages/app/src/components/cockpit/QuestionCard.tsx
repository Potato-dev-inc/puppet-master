// One `ask_user` question in the Inbox: the agent's question, its choices as buttons, a text field
// when free text is allowed, and Dismiss. It is an InboxCard like every other card.
import { useState } from 'react';
import { askerName, waitCaption, type Question, type QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { CardButton, InboxCard } from './InboxCard';

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
    <InboxCard
      kind="question"
      glyph="needs"
      title={q.question}
      meta={askerName(q, paneName)}
      age={now - q.created_at_ms}
      reason={q.context}
      note={<span data-testid="question-wait">{waitCaption(q, now, paneName)}</span>}
      data={{ 'data-question-id': q.id, 'data-urgent': q.urgency === 'high' ? '' : undefined }}
      body={
        q.allow_free_text ? (
          <form
            className="ib-field"
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
          >
            <input
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
        ) : null
      }
      actions={
        q.choices.length > 0 ? (
          // Equal weight on purpose: the agent wrote the choices, so none is made the easy default.
          <>
            {q.choices.map((c) => (
              <CardButton key={c.id} tone="secondary" role="answer" onClick={() => onAnswer({ choice_id: c.id })}>
                {c.label}
              </CardButton>
            ))}
          </>
        ) : null
      }
      more={
        <>
          <CardButton tone="plain" role="dismiss" onClick={onDismiss}>
            Dismiss
          </CardButton>
          {onOpenPane && (
            <CardButton tone="plain" role="open" onClick={onOpenPane}>
              Open terminal
            </CardButton>
          )}
        </>
      }
    />
  );
}
