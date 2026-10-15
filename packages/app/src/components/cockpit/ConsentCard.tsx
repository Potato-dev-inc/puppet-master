// A consent card in the Inbox: an agent wants to control a pane it did not start, or answer a risky
// prompt in one. It is a question Puppet Master raised (see src-tauri/src/agent_authority), so it
// arrives through the same list as `ask_user` questions and is answered the same way. Deny comes
// first and is the primary button on purpose: the easy tap is the safe one. No number key answers it,
// and there is no lock-screen version.
import { useEffect, useRef } from 'react';
import type { Question, QuestionAnswer } from '../../lib/questions';
import { waitCaption } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { CardButton, InboxCard, focusWithRing } from './InboxCard';

const UNKNOWN = 'Another agent';

/** "another agent" inside a sentence, a name as it is. */
const inSentence = (name: string): string => (name === UNKNOWN ? 'another agent' : name);

/** The card's question in the pane's own name: "Allow Claude Code to control Codex 1?" */
export function consentTitle(q: Question, paneName?: string): string {
  const info = q.consent;
  if (!info) return q.question;
  const who = inSentence(info.requester || UNKNOWN);
  const target = paneName?.trim() || info.target || 'that pane';
  return info.action === 'answer_prompt'
    ? `Allow ${who} to answer a question in ${target}?`
    : `Allow ${who} to control ${target}?`;
}

export function ConsentCard({
  q,
  paneName,
  now,
  onAnswer,
  onOpenPane,
  autoFocus = false,
}: {
  q: Question;
  /** The target pane's name in the sidebar, when it is open. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onOpenPane?: () => void;
  /** The first card in the Inbox: the keyboard starts on Deny. */
  autoFocus?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  const has = (id: string) => q.choices.some((c) => c.id === id);
  useEffect(() => {
    if (!autoFocus) return;
    const active = document.activeElement;
    if (active && active !== document.body && /^(INPUT|TEXTAREA)$/.test(active.tagName)) return;
    focusWithRing(root.current?.querySelector<HTMLElement>('[data-choice="deny"]'));
  }, [autoFocus, q.id]);

  return (
    <div ref={root} className="ib-approval">
      <InboxCard
        kind="consent"
        glyph="needs"
        guard
        title={consentTitle(q, paneName)}
        age={now - q.created_at_ms}
        reason={q.context}
        note={<span data-testid="consent-wait">{waitCaption(q, now)}</span>}
        data={{ 'data-consent-id': q.id }}
        actions={
          <>
            {has('deny') && (
              <CardButton tone="primary" role="answer" data-choice="deny" onClick={() => onAnswer({ choice_id: 'deny' })}>
                Deny
              </CardButton>
            )}
            {has('allow_once') && (
              <CardButton tone="secondary" role="answer" data-choice="allow_once" onClick={() => onAnswer({ choice_id: 'allow_once' })}>
                Allow once
              </CardButton>
            )}
            {has('always') && (
              <CardButton tone="plain" role="answer" data-choice="always" onClick={() => onAnswer({ choice_id: 'always' })}>
                Always for this pane
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
    </div>
  );
}
