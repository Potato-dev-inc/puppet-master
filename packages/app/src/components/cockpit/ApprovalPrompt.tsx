// Plain-language approval: what will happen, how risky it is, and the answers. Monochrome: risk is
// shown by glyph, weight and wording, never colour.
import { useEffect, useRef, useState } from 'react';
import type { PanePrompt, PromptChoice, RiskLevel } from '../../lib/cockpit-api';
import { CAUTION, LEVEL_WORD, firstReason, layoutChoices, riskGlyph } from '../../lib/approval-risk';
import '../../styles/approval-card.css';

function Glyph({ level }: { level: RiskLevel }) {
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

export function ApprovalPrompt({
  prompt,
  onAnswer,
  variant,
  autoFocus = true,
}: {
  prompt: PanePrompt;
  onAnswer: (choice: PromptChoice) => void;
  variant: 'card' | 'bar';
  autoFocus?: boolean;
}) {
  const risk = prompt.risk;
  const [details, setDetails] = useState(false);
  const [more, setMore] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const layout = layoutChoices(prompt);
  const high = risk?.level === 'high';

  // High risk: keyboard focus starts on Deny (or the details button), never on Allow, so a reflex
  // Enter cannot approve. Focus is not stolen from a text field or the terminal.
  useEffect(() => {
    if (!high || !autoFocus) return;
    const active = document.activeElement;
    if (active && active !== document.body && /^(INPUT|TEXTAREA)$/.test(active.tagName)) return;
    const target = layout.focusId
      ? Array.from(root.current?.querySelectorAll<HTMLElement>('[data-choice]') ?? []).find((el) => el.dataset.choice === layout.focusId)
      : root.current?.querySelector<HTMLElement>('[data-role="details"]');
    target?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prompt.promptId, high]);

  if (!risk) return null;
  const command = (prompt.summary || prompt.text || '').trim();
  const button = (ch: PromptChoice) => (
    <button
      key={ch.id}
      type="button"
      data-choice={ch.id}
      className={`ck-btn ck-btn--sm ${layout.primaryId === ch.id ? 'ck-btn--primary' : 'ck-btn--secondary'}`}
      onClick={() => onAnswer(ch)}
    >
      {ch.label}
    </button>
  );

  return (
    <div ref={root} className={`ck-appr ck-appr--${risk.level} ck-appr--${variant}`} role="group" aria-label="Approval request">
      <div className="ck-appr-head">
        <span className="ck-appr-glyph"><Glyph level={risk.level} /></span>
        <span className="ck-appr-headline">{risk.headline}</span>
        <span className="ck-appr-level">{LEVEL_WORD[risk.level]}</span>
      </div>
      {firstReason(risk) && <p className="ck-appr-reason">{firstReason(risk)}</p>}
      {high && <p className="ck-appr-caution">{CAUTION}</p>}
      <button type="button" data-role="details" className="ck-appr-toggle" aria-expanded={details} onClick={() => setDetails((d) => !d)}>
        {details ? 'Hide details' : 'Show details'}
      </button>
      {details && (
        <div className="ck-appr-details">
          {command && <pre className="ck-appr-cmd">{command}</pre>}
          {risk.reasons.length > 0 && (
            <ul>
              {risk.reasons.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      <div className="ck-appr-actions">
        {layout.visible.map(button)}
        {layout.more.length > 0 && (
          <button type="button" className="ck-btn ck-btn--sm ck-btn--plain" aria-expanded={more} onClick={() => setMore((m) => !m)}>
            More options
          </button>
        )}
      </div>
      {more && layout.more.length > 0 && <div className="ck-appr-actions ck-appr-more">{layout.more.map(button)}</div>}
    </div>
  );
}
