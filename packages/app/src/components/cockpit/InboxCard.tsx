// The ONE Inbox card. Every kind of thing that needs the person (an approval, a question from an
// agent, a consent, a budget pause, a usage limit, an error, a stuck agent, a note, a team update)
// is this shell, so the eye lands in the same place on every card:
//
//   row 1   glyph · what it wants, verb first · project            risk word · age
//   row 2   one reason line, then (high risk) the caution line
//   row 3   ONE action row: the answers on the left, the plain buttons (Open terminal, Dismiss) on the right
//
// Kind-specific content goes in `body`. Cards sit in a list 12px apart; the list is ordered, not grouped.
import type { ButtonHTMLAttributes, ReactNode } from 'react';
import type { RiskLevel } from '../../lib/cockpit-api';
import { riskGlyph } from '../../lib/approval-risk';
import '../../styles/inbox-card.css';
import { DotSpinner } from './DotSpinner';
import { agePhrase, formatAge } from './inbox-age';

/** `needs` waits for a person, `error` stopped, `notice` is calm information, `done` finished, the risk levels are approvals. */
export type CardGlyphKind = 'needs' | 'error' | 'notice' | 'working' | 'done' | RiskLevel;

export function RiskGlyph({ level }: { level: RiskLevel }) {
  const kind = riskGlyph(level);
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {kind === 'ring' && <circle cx="8" cy="8" r="5.5" />}
      {kind === 'triangle' && <path d="M8 2.5 14 13H2L8 2.5ZM8 6.5v3" />}
      {kind === 'stop' && (
        <>
          <path d="M5.2 2.5h5.6l2.7 2.7v5.6l-2.7 2.7H5.2l-2.7-2.7V5.2l2.7-2.7Z" fill="currentColor" />
          <path d="M8 5.5v3M8 10.6v.1" stroke="var(--bg)" />
        </>
      )}
    </svg>
  );
}

function CardGlyph({ kind }: { kind: CardGlyphKind }) {
  if (kind === 'low' || kind === 'medium' || kind === 'high') return <RiskGlyph level={kind} />;
  const status = kind === 'needs' ? 'waiting_input' : kind === 'error' ? 'error' : kind === 'working' ? 'running' : kind === 'done' ? 'exited' : 'idle';
  return <DotSpinner status={status} />;
}

export interface InboxCardProps {
  /** `data-kind`: approval, question, consent, budget, limit, error, stuck, note, team. */
  kind: string;
  glyph: CardGlyphKind;
  /** What it wants, verb first. The only bold line. */
  title: ReactNode;
  /** The project (or the agent and project), quiet, next to the title. */
  meta?: string | null;
  /** A word before the age, at the right edge: "High risk". */
  status?: string | null;
  /** How long it has waited, in ms. Shown in the one age format. */
  age?: number | null;
  /** The approval's risk: high gets a rule on the left edge and a heavier title. */
  risk?: RiskLevel;
  /** The card tells the person something; it asks for no decision, so the title is lighter. */
  info?: boolean;
  /** No answer may be picked with a number key (high risk and consent). */
  guard?: boolean;
  /** One line, label-2. */
  reason?: ReactNode;
  /** Only on high risk: label-1, 600. */
  caution?: ReactNode;
  /** Kind-specific content between the reason and the buttons (details, a text field, evidence lines). */
  body?: ReactNode;
  /** A small line under the body: how long the agent keeps waiting. */
  note?: ReactNode;
  /** The answers, left. Primary first, then secondary, then plain. */
  actions?: ReactNode;
  /** Plain buttons that are not answers (Open terminal, Dismiss, Details), right. */
  more?: ReactNode;
  /** Extra attributes for tests and deep links (`data-consent-id`). */
  data?: Record<string, string | undefined>;
  /** Content outside the card's grid (a sheet that portals out). */
  children?: ReactNode;
}

export function InboxCard({ kind, glyph, title, meta, status, age, risk, info, guard, reason, caution, body, note, actions, more, data, children }: InboxCardProps) {
  const extra = Object.fromEntries(Object.entries(data ?? {}).filter(([, v]) => v !== undefined));
  const hasAge = age !== undefined && age !== null;
  return (
    <section
      className="ib-card"
      role="group"
      data-ib-card=""
      data-kind={kind}
      data-risk={risk}
      data-info={info ? '' : undefined}
      data-guard={guard ? '' : undefined}
      tabIndex={-1}
      {...extra}
    >
      <span className="ib-glyph" aria-hidden={glyph === 'notice' ? true : undefined}>
        <CardGlyph kind={glyph} />
      </span>
      <div className="ib-main">
        <span className="ib-title">{title}</span>
        {meta ? <span className="ib-meta">{meta}</span> : null}
      </div>
      {(status || hasAge) && (
        <div className="ib-aside">
          {status ? <span className="ib-status">{status}</span> : null}
          {hasAge ? (
            <span className="ib-age" title={agePhrase(age)} aria-label={agePhrase(age)}>
              {formatAge(age)}
            </span>
          ) : null}
        </div>
      )}
      {(reason || caution || body || note) && (
        <div className="ib-body">
          {reason ? <p className="ib-reason">{reason}</p> : null}
          {caution ? <p className="ib-caution">{caution}</p> : null}
          {body}
          {note ? <p className="ib-note">{note}</p> : null}
        </div>
      )}
      {(actions || more) && (
        <div className="ib-actions">
          {actions}
          {more ? <span className="ib-actions-end">{more}</span> : null}
        </div>
      )}
      {children}
    </section>
  );
}

type Tone = 'primary' | 'secondary' | 'plain';

/**
 * A card button. `role` marks what keyboard shortcuts may do with it: `answer` (1, 2, 3), `open` (O),
 * `dismiss` (Esc). Everything else is a plain button.
 */
export function CardButton({
  tone,
  role,
  size,
  children,
  ...rest
}: {
  tone: Tone;
  role?: 'answer' | 'open' | 'dismiss';
  size?: 'sm';
  children: ReactNode;
} & Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'className' | 'role'>) {
  return (
    <button type="button" className={`ck-btn ck-btn--${tone}${size ? ` ck-btn--${size}` : ''}`} data-ib={role} {...rest}>
      {children}
    </button>
  );
}

/** A command or scope inside a sentence or a button: inline code, never backticks. */
export function InlineCode({ children }: { children: ReactNode }) {
  return <code className="ib-code">{children}</code>;
}

/**
 * Focus an element from code and show the focus ring, which the browser hides for focus that did
 * not come from the keyboard. The ring is what tells a person where Enter will go.
 */
export function focusWithRing(el: HTMLElement | null | undefined): void {
  if (!el) return;
  el.focus();
  if (typeof document === 'undefined' || document.activeElement !== el) return;
  el.setAttribute('data-ring', '');
  const off = () => {
    el.removeAttribute('data-ring');
    el.removeEventListener('blur', off);
  };
  el.addEventListener('blur', off);
}
