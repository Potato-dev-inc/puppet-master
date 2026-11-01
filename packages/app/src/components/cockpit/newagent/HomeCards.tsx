// The cards under the New agent box: what the person pinned (recent projects, running agents, what needs them, a
// saved prompt). "+ Add card" pins one, "Customize" shows move and remove buttons. The list is saved in settings by
// the page (`onChange`); this file only edits it in memory.
import { useState } from 'react';
import type { RecentProject } from '../../../lib/ship-api';
import { stateHint } from '../../../lib/ship-model';
import {
  CARD_KINDS,
  MAX_PROMPT_CHARS,
  addCard,
  canAdd,
  cardTitle,
  moveCard,
  removeCard,
  type CardKind,
  type HomeCard,
} from './model';

/** A saved prompt shows its own text in the body, so its heading is the plain name. */
const cardHeading = (card: HomeCard): string => (card.kind === 'prompt' ? 'Saved prompt' : cardTitle(card));

interface PaneLine {
  id: string;
  title: string;
  status: string;
}

/** What the cards show. */
export interface CardData {
  recent: readonly RecentProject[];
  running: readonly PaneLine[];
  needs: readonly PaneLine[];
}

export function HomeCards({
  cards,
  data,
  onChange,
  onUseFolder,
  onOpenPane,
  onUsePrompt,
}: {
  cards: readonly HomeCard[];
  data: CardData;
  onChange: (cards: HomeCard[]) => void;
  onUseFolder: (project: RecentProject) => void;
  onOpenPane: (paneId: string) => void;
  onUsePrompt: (text: string) => void;
}) {
  const [customize, setCustomize] = useState(false);
  const [adding, setAdding] = useState<'list' | 'prompt' | null>(null);
  const [draft, setDraft] = useState('');
  const kinds = CARD_KINDS.filter((k) => canAdd(cards, k.kind));

  const pin = (kind: CardKind, text?: string) => {
    onChange(addCard(cards, kind, text));
    setAdding(null);
    setDraft('');
  };

  return (
    <div className="ck-na-cards">
      {cards.length > 0 && (
        <div className="ck-na-grid">
          {cards.map((card, i) => (
            <section key={card.id} className="ck-na-hc" aria-label={cardHeading(card)}>
              <header className="ck-na-hc-head">
                <h2 className="ck-na-hc-title">{cardHeading(card)}</h2>
                {customize && (
                  <span className="ck-na-hc-tools">
                    <button type="button" className="ck-na-mini" aria-label={`Move ${cardTitle(card)} up`} disabled={i === 0} onClick={() => onChange(moveCard(cards, card.id, -1))}>
                      ↑
                    </button>
                    <button type="button" className="ck-na-mini" aria-label={`Move ${cardTitle(card)} down`} disabled={i === cards.length - 1} onClick={() => onChange(moveCard(cards, card.id, 1))}>
                      ↓
                    </button>
                    <button type="button" className="ck-na-mini" aria-label={`Remove ${cardTitle(card)}`} onClick={() => onChange(removeCard(cards, card.id))}>
                      ×
                    </button>
                  </span>
                )}
              </header>
              <CardBody card={card} data={data} onUseFolder={onUseFolder} onOpenPane={onOpenPane} onUsePrompt={onUsePrompt} />
            </section>
          ))}
        </div>
      )}

      {adding === 'list' && (
        <div className="ck-na-add-list" role="menu" aria-label="Add a card">
          {kinds.map((k) => (
            <button
              key={k.kind}
              type="button"
              role="menuitem"
              className="ck-na-item"
              onClick={() => (k.kind === 'prompt' ? setAdding('prompt') : pin(k.kind))}
            >
              <span>{k.label}</span>
              <span className="ck-na-item-hint">{k.hint}</span>
            </button>
          ))}
          {kinds.length === 0 && <p className="ck-na-note">All the cards are already pinned.</p>}
        </div>
      )}

      {adding === 'prompt' && (
        <div className="ck-na-add-prompt">
          <textarea
            className="ck-na-input ck-na-input--small"
            aria-label="Saved prompt text"
            placeholder="Text to keep, for example how you like commits written."
            maxLength={MAX_PROMPT_CHARS}
            rows={3}
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          <div className="ck-na-add-actions">
            <button type="button" className="ck-btn ck-btn--primary ck-btn--sm" disabled={!draft.trim()} onClick={() => pin('prompt', draft)}>
              Pin
            </button>
            <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => setAdding(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="ck-na-foot">
        {adding === null && kinds.length > 0 && (
          <button type="button" className="ck-na-addrow" onClick={() => setAdding('list')}>
            <span aria-hidden="true">+</span> Add card
          </button>
        )}
        {adding !== null && (
          <button type="button" className="ck-na-addrow" onClick={() => setAdding(null)}>
            Close
          </button>
        )}
        {cards.length > 0 && (
          <button type="button" className="ck-na-link" aria-pressed={customize} onClick={() => setCustomize((v) => !v)}>
            {customize ? 'Done' : 'Customize'}
          </button>
        )}
      </div>
    </div>
  );
}

function CardBody({
  card,
  data,
  onUseFolder,
  onOpenPane,
  onUsePrompt,
}: {
  card: HomeCard;
  data: CardData;
  onUseFolder: (project: RecentProject) => void;
  onOpenPane: (paneId: string) => void;
  onUsePrompt: (text: string) => void;
}) {
  if (card.kind === 'prompt') {
    return (
      <button type="button" className="ck-na-prompt" title="Use this text" onClick={() => onUsePrompt(card.text ?? '')}>
        {card.text}
      </button>
    );
  }
  if (card.kind === 'recent') {
    const list = data.recent.filter((p) => p.state !== 'missing').slice(0, 5);
    if (list.length === 0) return <p className="ck-na-note">No projects yet.</p>;
    return (
      <ul className="ck-na-list">
        {list.map((p) => (
          <li key={p.path}>
            <button type="button" className="ck-na-line" title={p.path} onClick={() => onUseFolder(p)}>
              <span className="ck-na-line-main">{p.name}</span>
              {stateHint(p.state) && <span className="ck-na-line-side">{stateHint(p.state)}</span>}
            </button>
          </li>
        ))}
      </ul>
    );
  }
  const lines = card.kind === 'running' ? data.running : data.needs;
  if (lines.length === 0) return <p className="ck-na-note">{card.kind === 'running' ? 'Nothing is running.' : 'Nothing is waiting for you.'}</p>;
  return (
    <ul className="ck-na-list">
      {lines.slice(0, 6).map((p) => (
        <li key={p.id}>
          <button type="button" className="ck-na-line" onClick={() => onOpenPane(p.id)}>
            <span className="ck-na-line-main">{p.title}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
