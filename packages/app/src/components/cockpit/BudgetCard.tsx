// The pause card in the Inbox: an agent used its budget and waits before its next turn. It is a
// question Puppet Master raised (src-tauri/src/budgets), so it arrives through the same list as
// `ask_user` questions. The agent is not killed and nothing is lost; the person adds turns or
// stops it here. Only the person can answer it (the bridge refuses a plain loopback answer).
import type { Question, QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { DotSpinner } from './DotSpinner';
import { formatElapsed } from './model';

export function BudgetCard({
  q,
  paneName,
  now,
  onAnswer,
  onDismiss,
  onOpenPane,
}: {
  q: Question;
  /** The pane's name in the sidebar, when it has one. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onDismiss: () => void;
  onOpenPane?: () => void;
}) {
  const stop = q.choices.find((c) => c.id === 'stop');
  const adds = q.choices.filter((c) => c.id !== 'stop');
  return (
    <li className="ck-inbox-item" data-budget-id={q.id}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status="waiting_input" />
        </span>
        <span className="ck-row-title">{q.question}</span>
        {paneName && <span className="ck-row-sub">{paneName}</span>}
        <span className="ck-row-time">{`${formatElapsed(now - q.created_at_ms)} ago`}</span>
      </div>
      {q.context && <p className="ck-q-ctx">{q.context}</p>}
      <div className="ck-inbox-actions">
        {adds.map((c, i) => (
          <button
            key={c.id}
            type="button"
            className={`ck-btn ${i === 0 ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
            onClick={() => onAnswer({ choice_id: c.id })}
          >
            {c.label}
          </button>
        ))}
        {stop && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => onAnswer({ choice_id: stop.id })}>
            {stop.label}
          </button>
        )}
        {onOpenPane ? (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenPane}>
            Open
          </button>
        ) : (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onDismiss}>
            Dismiss
          </button>
        )}
      </div>
    </li>
  );
}
