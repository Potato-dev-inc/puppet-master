// Plain-language approval: what will happen, how risky it is, and the answers. Monochrome: risk is
// shown by glyph, weight and wording, never colour.
//
// The `card` variant is an InboxCard. The answers always read "Allow once", "Always allow <command>"
// and "Deny", whatever the agent's own menu says (the agent's words stay in the tooltip), and on high
// risk Deny comes first, is the primary button and is where the keyboard starts.
import { useEffect, useRef, useState } from 'react';
import type { PanePrompt, PromptChoice } from '../../lib/cockpit-api';
import { CAUTION, LEVEL_WORD, choiceWords, firstReason, isStartupGate, layoutChoices } from '../../lib/approval-risk';
import { gestureOf, type UserGesture } from '../../lib/user-gesture';
import '../../styles/approval-card.css';
import { CardButton, InboxCard, InlineCode, RiskGlyph, focusWithRing } from './InboxCard';
import { PackageFacts } from './PackageFacts';

export function ApprovalPrompt({
  prompt,
  onAnswer,
  variant,
  paneId,
  autoFocus = true,
  who,
  age,
  onOpen,
  asks,
  openLabel = 'Open terminal',
}: {
  prompt: PanePrompt;
  /** `gesture` is the click that chose it (`lib/user-gesture.ts`); the backend needs it. */
  onAnswer: (choice: PromptChoice, gesture: UserGesture) => void;
  variant: 'card' | 'bar';
  /** The pane that shows the prompt. Package check looks up its packages only when this is set. */
  paneId?: string;
  /** High risk only: move the keyboard to Deny when the card appears. */
  autoFocus?: boolean;
  /** Card: who is asking and where, e.g. "Codex · billing-service". */
  who?: string;
  /** Card: how long it has waited, in ms. */
  age?: number | null;
  /** Card: the "Open terminal" button. */
  onOpen?: () => void;
  /** Card: a helper's question shown in its parent's Chat: "Claude Code helper asks" leads the title. */
  asks?: string;
  /** Card: the open button's words. */
  openLabel?: string;
}) {
  const risk = prompt.risk;
  const [details, setDetails] = useState(false);
  const [more, setMore] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const layout = layoutChoices(prompt);
  // A startup gate and the agent's own question keep the agent's words ("Red", "Trust this folder"): "Allow once" would be wrong.
  const asksQuestion = prompt.kind === 'question';
  const words = isStartupGate(prompt.kind) || asksQuestion ? new Map<string, { text: string; code?: string }>() : choiceWords(prompt.choices);
  const high = risk?.level === 'high';

  // High risk: keyboard focus starts on Deny (or the details button), never on Allow, so a reflex
  // Enter cannot approve, and a ring shows where Enter will go. Focus is not stolen from a text field.
  useEffect(() => {
    if (!high || !autoFocus) return;
    const active = document.activeElement;
    if (active && active !== document.body && /^(INPUT|TEXTAREA)$/.test(active.tagName)) return;
    const scope: ParentNode | null = root.current;
    const target = layout.focusId
      ? Array.from(scope?.querySelectorAll<HTMLElement>('[data-choice]') ?? []).find((el) => el.dataset.choice === layout.focusId)
      : scope?.querySelector<HTMLElement>('[data-role="details"]');
    focusWithRing(target);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prompt.promptId, high]);

  if (!risk) return null;
  const command = (prompt.summary || prompt.text || '').trim();
  // The first reason is already on the card; Details adds what it does not repeat.
  const furtherReasons = risk.reasons.slice(1);
  // Package check shows when the command installs something: its rows sit on the card, its facts in Details.
  const hasPackages = (prompt.packages?.items.length ?? 0) > 0 || (prompt.packages?.hidden ?? 0) > 0;
  const hasDetails = command.length > 0 || furtherReasons.length > 0 || hasPackages;
  const small = variant === 'bar';
  const cardTitle = asksQuestion && prompt.summary ? prompt.summary : risk.headline;

  const button = (ch: PromptChoice) => {
    const w = words.get(ch.id) ?? { text: ch.label };
    return (
      <CardButton
        key={ch.id}
        tone={layout.primaryId === ch.id ? 'primary' : 'secondary'}
        role="answer"
        size={small ? 'sm' : undefined}
        data-choice={ch.id}
        title={w.text !== ch.label ? ch.label : undefined}
        onClick={(event) => onAnswer(ch, gestureOf(event))}
      >
        {w.code ? (
          <>
            {w.text} <InlineCode>{w.code}</InlineCode>
          </>
        ) : (
          w.text
        )}
      </CardButton>
    );
  };

  const packageFacts = hasPackages && <PackageFacts paneId={paneId} prompt={prompt} details={details} variant={small ? 'bar' : 'card'} />;

  const detailsPanel = details && hasDetails && (
    <div className={variant === 'card' ? 'ib-details' : 'ck-appr-details'}>
      {command && <pre className={variant === 'card' ? 'ib-cmd' : 'ck-appr-cmd'}>{command}</pre>}
      {furtherReasons.length > 0 && (
        <ul>
          {furtherReasons.map((r) => (
            <li key={r}>{r}</li>
          ))}
        </ul>
      )}
    </div>
  );

  const moreToggle = layout.more.length > 0 && (
    <CardButton tone="plain" size={small ? 'sm' : undefined} aria-expanded={more} onClick={() => setMore((m) => !m)}>
      More options
    </CardButton>
  );

  if (variant === 'card') {
    return (
      <div ref={root} className="ib-approval">
        <InboxCard
          kind="approval"
          glyph={risk.level}
          risk={risk.level}
          guard={high}
          title={asks ? `${asks}: ${cardTitle}` : cardTitle}
          meta={who}
          status={asksQuestion ? null : LEVEL_WORD[risk.level]}
          age={age ?? null}
          reason={firstReason(risk) || null}
          caution={high ? CAUTION : null}
          body={
            packageFacts || detailsPanel ? (
              <>
                {packageFacts}
                {detailsPanel}
              </>
            ) : null
          }
          actions={
            <>
              {layout.visible.map(button)}
              {moreToggle}
              {more && layout.more.map(button)}
            </>
          }
          more={
            <>
              {hasDetails && (
                <CardButton tone="plain" data-role="details" aria-expanded={details} onClick={() => setDetails((d) => !d)}>
                  {details ? 'Hide details' : 'Details'}
                </CardButton>
              )}
              {onOpen && (
                <CardButton tone="plain" role="open" onClick={onOpen}>
                  {openLabel}
                </CardButton>
              )}
            </>
          }
        />
      </div>
    );
  }

  // The bar docked inside a pane keeps its own compact shape.
  return (
    <div ref={root} className={`ck-appr ck-appr--${risk.level} ck-appr--bar`} role="group" aria-label="Approval request">
      <div className="ck-appr-head">
        <span className="ck-appr-glyph">
          <RiskGlyph level={risk.level} />
        </span>
        <span className="ck-appr-headline">{risk.headline}</span>
        <span className="ck-appr-level">{LEVEL_WORD[risk.level]}</span>
      </div>
      {firstReason(risk) && <p className="ck-appr-reason">{firstReason(risk)}</p>}
      {high && <p className="ck-appr-caution">{CAUTION}</p>}
      {packageFacts}
      {hasDetails && (
        <button type="button" data-role="details" className="ck-appr-toggle" aria-expanded={details} onClick={() => setDetails((d) => !d)}>
          {details ? 'Hide details' : 'Show details'}
        </button>
      )}
      {detailsPanel}
      <div className="ck-appr-actions">
        {layout.visible.map(button)}
        {moreToggle}
      </div>
      {more && layout.more.length > 0 && <div className="ck-appr-actions ck-appr-more">{layout.more.map(button)}</div>}
    </div>
  );
}
