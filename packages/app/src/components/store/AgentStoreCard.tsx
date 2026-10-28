// One Agent Store card: name, one-line tagline, tier and license chips, the state glyph, the reason and one button.
// Every text comes from the catalog as data: it is shown as plain text (React escapes it), never as HTML, and no link is made.
import type { StoreCard, StoreState } from '../../lib/agent-store';
import { licenseWords, primaryButton, tierWords } from '../../lib/agent-store-model';
import { DotSpinner, type DotSpinnerProps } from '../cockpit/DotSpinner';

/** The glyph reuses the doctor's DotSpinner statics: check = ready, plus = needs you, cross = a problem. */
export function stateGlyph(state: StoreState): DotSpinnerProps['status'] {
  switch (state) {
    case 'ready':
      return 'exited';
    case 'needs_review':
      return 'waiting_input';
    case 'added_no_program':
      return 'error';
    default:
      return 'idle';
  }
}

/** The words a screen reader says for the glyph. */
export function stateWords(state: StoreState): string {
  switch (state) {
    case 'ready':
      return 'Ready';
    case 'found_on_pc':
      return 'Found on this PC';
    case 'added_no_program':
      return 'Added, but the program is missing';
    case 'needs_review':
      return 'Needs your review';
    case 'own_version':
      return 'You have your own version';
    case 'unsupported':
      return 'Not for this PC';
    default:
      return 'Not installed';
  }
}

export interface AgentStoreCardProps {
  card: StoreCard;
  /** The card's one button calls this with the agent id. The grid installs nothing itself. */
  onOpen: (agentId: string) => void;
}

export function AgentStoreCard({ card, onOpen }: AgentStoreCardProps) {
  const button = primaryButton(card);
  return (
    <article className="ck-store-card" data-state={card.state} data-tier={card.tier}>
      <div className="ck-store-card-head">
        <span className="ck-store-icon" aria-hidden="true">
          {card.icon}
        </span>
        <div className="ck-store-card-title">
          <span className="ck-row-title">{card.label}</span>
          <p className="ck-store-tagline">{card.tagline}</p>
        </div>
        <span className="ck-store-glyph">
          <DotSpinner status={stateGlyph(card.state)} label={stateWords(card.state)} />
        </span>
      </div>

      <div className="ck-store-tags">
        <span className="ck-store-tag" data-tier={card.tier}>
          {tierWords(card)}
        </span>
        <span className="ck-store-tag">{licenseWords(card)}</span>
        {card.modes.length > 0 && <span className="ck-store-modes">{card.modes.join(' · ')}</span>}
      </div>

      <p className="ck-store-reason">{card.state_reason}</p>

      {button.kind !== 'none' && (
        <div className="ck-store-actions">
          <button
            type="button"
            className="ck-btn ck-btn--primary ck-btn--sm"
            aria-label={`${button.label} ${card.label}`}
            onClick={() => onOpen(card.id)}
          >
            {button.label}
          </button>
        </div>
      )}
    </article>
  );
}
