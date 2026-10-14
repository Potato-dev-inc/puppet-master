// A consent card in the Inbox: an agent wants to control a pane it did not start, or answer a risky
// prompt in one. It is a question Puppet Master raised (see src-tauri/src/agent_authority), so it
// arrives through the same list as `ask_user` questions and is answered the same way. Deny is the
// primary button on purpose: the easy tap is the safe one. There is no lock-screen version.
import type { Question, QuestionAnswer } from '../../lib/questions';
import { waitCaption } from '../../lib/questions';
import '../../styles/cockpit-questions.css';
import { DotSpinner } from './DotSpinner';
import { formatElapsed } from './model';

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
}: {
  q: Question;
  /** The target pane's name in the sidebar, when it is open. */
  paneName?: string;
  now: number;
  onAnswer: (answer: QuestionAnswer) => void;
  onOpenPane?: () => void;
}) {
  const has = (id: string) => q.choices.some((c) => c.id === id);
  return (
    <li className="ck-inbox-item is-urgent" data-consent-id={q.id}>
      <div className="ck-inbox-top">
        <span className="ck-row-glyph">
          <DotSpinner status="waiting_input" />
        </span>
        <span className="ck-row-title">{consentTitle(q, paneName)}</span>
        <span className="ck-row-time">{`asked ${formatElapsed(now - q.created_at_ms)} ago`}</span>
      </div>
      {q.context && <p className="ck-q-ctx">{q.context}</p>}
      <div className="ck-inbox-actions">
        {has('deny') && (
          <button type="button" className="ck-btn ck-btn--primary" onClick={() => onAnswer({ choice_id: 'deny' })}>
            Deny
          </button>
        )}
        {has('allow_once') && (
          <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onAnswer({ choice_id: 'allow_once' })}>
            Allow once
          </button>
        )}
        {has('always') && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={() => onAnswer({ choice_id: 'always' })}>
            Always for this pane
          </button>
        )}
        {onOpenPane && (
          <button type="button" className="ck-btn ck-btn--plain" onClick={onOpenPane}>
            Open terminal
          </button>
        )}
      </div>
      <p className="ck-q-wait" data-testid="consent-wait">
        {waitCaption(q, now)}
      </p>
    </li>
  );
}
