// The pause card in the Inbox: an agent used its budget and waits before its next turn. It is a
// question Puppet Master raised (src-tauri/src/budgets), so it arrives through the same list as
// `ask_user` questions. The agent is not killed and nothing is lost; the person adds turns or
// stops it here. Only the person can answer it (the bridge refuses a plain loopback answer).
import type { Question, QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { CardButton, InboxCard } from './InboxCard';

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
    <InboxCard
      kind="budget"
      glyph="needs"
      title={q.question}
      meta={paneName}
      age={now - q.created_at_ms}
      reason={q.context}
      data={{ 'data-budget-id': q.id }}
      actions={
        <>
          {adds.map((c, i) => (
            <CardButton key={c.id} tone={i === 0 ? 'primary' : 'secondary'} role="answer" onClick={() => onAnswer({ choice_id: c.id })}>
              {c.label}
            </CardButton>
          ))}
          {stop && (
            <CardButton tone="plain" role="answer" onClick={() => onAnswer({ choice_id: stop.id })}>
              {stop.label}
            </CardButton>
          )}
        </>
      }
      more={
        onOpenPane ? (
          <CardButton tone="plain" role="open" onClick={onOpenPane}>
            Open terminal
          </CardButton>
        ) : (
          <CardButton tone="plain" role="dismiss" onClick={onDismiss}>
            Dismiss
          </CardButton>
        )
      }
    />
  );
}
