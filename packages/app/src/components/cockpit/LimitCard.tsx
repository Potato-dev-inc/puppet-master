// A usage-limit card in the Inbox: an agent ran out of usage. It is a question Puppet Master raised (see
// src-tauri/src/handoff), so it arrives through the same list as `ask_user` questions and is answered the same way.
//
// Detect-only: two choices, "Remind me at 14:05" and "Stop here". Neither presses anything in the agent's pane.
// There is no hand-off button yet; the card says that is coming. There is no lock-screen version.
import { useState } from 'react';
import type { CompanionTransport, CompanionView } from '../../lib/companion';
import type { Question, QuestionAnswer } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { WakeOnSheet } from './companion/WakeOnSheet';
import { CardButton, InboxCard } from './InboxCard';

export function LimitCard({
  q,
  paneName,
  now,
  onAnswer,
  onOpenPane,
  companion,
  companionTransport,
}: {
  q: Question;
  /** The pane's name in the sidebar, when it is open. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onOpenPane?: () => void;
  /** The pane is a companion's: the card then offers "Wake {name} on…" (it runs on another agent). */
  companion?: CompanionView;
  companionTransport?: Pick<CompanionTransport, 'agents' | 'update'>;
}) {
  const [waking, setWaking] = useState(false);
  const first = q.choices[0];
  const rest = q.choices.slice(1);
  const canMove = companion !== undefined && companionTransport !== undefined && q.limit?.kind === 'usage_limit';
  return (
    <>
      <InboxCard
        kind="limit"
        glyph="needs"
        title={q.question}
        meta={paneName}
        age={now - q.created_at_ms}
        reason={q.context}
        data={{ 'data-limit-id': q.id, 'data-limit-kind': q.limit?.kind }}
        actions={
          <>
            {first && (
              <CardButton tone="primary" role="answer" onClick={() => onAnswer({ choice_id: first.id })}>
                {first.label}
              </CardButton>
            )}
            {rest.map((c) => (
              <CardButton key={c.id} tone="plain" role="answer" onClick={() => onAnswer({ choice_id: c.id })}>
                {c.label}
              </CardButton>
            ))}
            {canMove && (
              <CardButton tone="plain" onClick={() => setWaking(true)}>
                Wake {companion.name} on…
              </CardButton>
            )}
          </>
        }
        more={
          onOpenPane ? (
            <CardButton tone="plain" role="open" onClick={onOpenPane}>
              Open terminal
            </CardButton>
          ) : null
        }
      />
      {waking && canMove && (
        <WakeOnSheet companion={companion} transport={companionTransport} resetAtMs={q.limit?.reset_at_ms} onClose={() => setWaking(false)} />
      )}
    </>
  );
}
