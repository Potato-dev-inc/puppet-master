// Agent Store: search, filter chips and a grid of agent cards. Lists what the catalog ships; installing happens in the detail sheet.
// The grid only reads the list (agentStoreApi.list) and hands a card's button to onOpenAgent.
import { useCallback, useEffect, useMemo, useState } from 'react';
import { agentStoreApi, type AgentStoreApi, type StoreCard } from '../../lib/agent-store';
import { matchesFilter, searchCards, sortCards, STORE_FILTERS, type StoreFilter } from '../../lib/agent-store-model';
import { AgentStoreCard } from './AgentStoreCard';
import '../../styles/agent-store.css';

export interface AgentStoreProps {
  api?: AgentStoreApi;
  /** A card's button was pressed for this agent id. */
  onOpenAgent: (agentId: string) => void;
  onBack: () => void;
}

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e)).replace(/^[A-Z_]+: /, '');

export function AgentStore({ api = agentStoreApi, onOpenAgent, onBack }: AgentStoreProps) {
  const [cards, setCards] = useState<StoreCard[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [text, setText] = useState('');
  const [filter, setFilter] = useState<StoreFilter>('all');

  const load = useCallback(() => {
    setLoadError(null);
    api
      .list()
      .then((list) => setCards(list.entries))
      .catch((e) => setLoadError(errText(e)));
  }, [api]);

  useEffect(load, [load]);

  const shown = useMemo(
    () => (cards ? sortCards(searchCards(cards.filter((card) => matchesFilter(card, filter)), text)) : []),
    [cards, filter, text],
  );

  return (
    <div className="ck-store">
      <header className="ck-store-head">
        <div>
          <h1 className="ck-title">Agent Store</h1>
          <p className="ck-sub">Agents you can add to Puppet Master. Each card says how well we have tested it.</p>
        </div>
        <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" onClick={onBack}>
          Back
        </button>
      </header>

      <div className="ck-store-bar">
        <input
          type="search"
          className="ck-store-search"
          aria-label="Search agents"
          placeholder="Search agents"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="ck-store-chips" role="group" aria-label="Filters">
          {STORE_FILTERS.map((f) => (
            <button
              key={f.id}
              type="button"
              aria-pressed={filter === f.id}
              className={filter === f.id ? 'is-on' : undefined}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {loadError && (
        <div className="ck-store-error" role="alert">
          <p>Could not load the Agent Store: {loadError}</p>
          <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={load}>
            Try again
          </button>
        </div>
      )}
      {!cards && !loadError && <p className="ck-sub">Loading agents</p>}

      {cards && shown.length > 0 && (
        <ul className="ck-store-grid" aria-label="Agents">
          {shown.map((card) => (
            <li key={card.id}>
              <AgentStoreCard card={card} onOpen={onOpenAgent} />
            </li>
          ))}
        </ul>
      )}
      {cards && shown.length === 0 && (
        <p className="ck-sub ck-store-empty">
          {cards.length === 0 ? 'The Store has no agents yet.' : 'No agents match. Try another word, or press All.'}
        </p>
      )}
    </div>
  );
}
