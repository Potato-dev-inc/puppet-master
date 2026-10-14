// A usage-limit card in the Inbox: an agent ran out of usage. It is a question Puppet Master raised (see
// src-tauri/src/handoff), so it arrives through the same list as `ask_user` questions and is answered the same way.
//
// Detect-only: two choices, "Remind me at 14:05" and "Stop here". Neither presses anything in the agent's pane.
// There is no hand-off button yet; the card says that is coming. There is no lock-screen version.
import type { Question, QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { DotSpinner } from './DotSpinner';
import { formatElapsed } from './model';

export function LimitCard({
  q,
  paneName,
  now,
  onAnswer,
  onOpenPane,
}: {
  q: Question;
  /** The pane's name in the sidebar, when it is open. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onOpenPane?: () => void;
}) {
  const reminder = q.limit?.kind === 'reset_reminder';
  const first = q.choices[0];
  const rest = q.choices.slice(1);
  return (
    <li className="ck-inbox-item" data-limit-id={q.id} data-limit-kind={q.limit?.kind}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status="waiting_input" />
        </span>
        <span className="ck-row-title">{q.question}</span>
        <span className="ck-row-time">{`${reminder ? 'reset' : 'ran out'} ${formatElapsed(now - q.created_at_ms)} ago`}</span>
      </div>
      {q.context && <p className="ck-q-ctx">{q.context}</p>}
      {paneName && <p className="ck-q-ctx" data-testid="limit-pane">{`In ${paneName}`}</p>}
      <div className="ck-inbox-actions">
        {first && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onAnswer({ choice_id: first.id })}>
            {first.label}
          </button>
        )}
        {rest.map((c) => (
          <button key={c.id} type="button" className="ck-btn ck-btn--plain" onClick={() => onAnswer({ choice_id: c.id })}>
            {c.label}
          </button>
        ))}
        {onOpenPane && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenPane}>
            Open terminal
          </button>
        )}
      </div>
    </li>
  );
}
