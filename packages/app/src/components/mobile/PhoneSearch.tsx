// "Search all chats" on the phone: a full-screen search from the top bar. Type a word and the PC answers with every
// chat that said it (who said it, the words found marked, agent · folder · time). A hit whose agent is still open
// opens that chat on the phone. A hit whose agent is closed shows only what was kept, read-only.
import { useState, type ReactNode } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../../lib/bridge';
import { kindText, snippetParts, whenText, type ChatHit } from '../../lib/chatSearch';
import { usePhoneChatSearch } from '../../hooks/usePhoneChatSearch';

export const SEARCH_OFF_TEXT = 'Search is off — turn it on in Settings on your computer';
/** The longest search the PC takes (`bridge/routes/chats.rs`). */
export const SEARCH_MAX_CHARS = 200;

function Snippet({ hit }: { hit: ChatHit }) {
  return (
    <>
      {snippetParts(hit.snippet, hit.marks).map((part, i) =>
        part.hit ? (
          <mark key={i} className="ph-search-mark">
            {part.text}
          </mark>
        ) : (
          <span key={i}>{part.text}</span>
        ),
      )}
    </>
  );
}

function metaOf(hit: ChatHit, agentName: (id: string) => string, now: number): string {
  return [agentName(hit.agent), hit.folder, whenText(hit.atMs, now)].filter(Boolean).join(' · ');
}

export function PhoneSearch({
  bridge,
  panes,
  agentName,
  onOpenChat,
  onClose,
}: {
  bridge: BridgeClient;
  /** The panes that are open now. A hit in one of them opens its chat; any other hit is read-only. */
  panes: PaneInfo[];
  /** The name of an agent id (`claude` -> "Claude"). */
  agentName: (agentId: string) => string;
  /** Opens this pane's chat on the phone (the screen closes). */
  onOpenChat: (paneId: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState('');
  const search = usePhoneChatSearch(bridge, query);
  /** A hit whose pane is closed: its snippet, read-only. */
  const [closedHit, setClosedHit] = useState<ChatHit | null>(null);
  const now = Date.now();
  const open = (hit: ChatHit) => {
    if (panes.some((pane) => pane.id === hit.paneId)) onOpenChat(hit.paneId);
    else setClosedHit(hit);
  };

  let body: ReactNode;
  if (closedHit) {
    body = (
      <div className="ph-search-detail">
        <p className="ph-search-detail-meta">
          {kindText(closedHit.kind)} · {metaOf(closedHit, agentName, now)}
        </p>
        <p className="ph-search-detail-text">
          <Snippet hit={closedHit} />
        </p>
        <p className="ph-search-note">This chat is closed, so this is all that was kept.</p>
        <button type="button" className="ph-btn" onClick={() => setClosedHit(null)}>
          Back to results
        </button>
      </div>
    );
  } else if (search.state === 'off') {
    body = <p className="ph-search-note">{SEARCH_OFF_TEXT}</p>;
  } else if (search.state === 'failed') {
    body = <p className="ph-search-note">Could not search. Check the connection to your PC and try again.</p>;
  } else if (!query.trim()) {
    body = <p className="ph-search-note">Type a word to find it in every chat on this PC.</p>;
  } else if (search.state === 'done' && search.hits.length === 0) {
    body = <p className="ph-search-note">No chat says &ldquo;{query.trim()}&rdquo;.</p>;
  } else if (search.hits.length === 0) {
    body = <p className="ph-search-note">Searching…</p>;
  } else {
    body = (
      <ul className="ph-search-list" aria-label="Results">
        {search.hits.map((hit) => {
          const live = panes.some((pane) => pane.id === hit.paneId);
          return (
            <li key={`${hit.paneId}:${hit.session}:${hit.itemId}`}>
              <button type="button" className="ph-search-hit" onClick={() => open(hit)}>
                <span className="ph-search-who">
                  {kindText(hit.kind)}
                  {!live && <span className="ph-search-closed">Closed</span>}
                </span>
                <span className="ph-search-text">
                  <Snippet hit={hit} />
                </span>
                <span className="ph-search-meta">{metaOf(hit, agentName, now)}</span>
              </button>
            </li>
          );
        })}
      </ul>
    );
  }

  return (
    <section className="ph-search" role="dialog" aria-modal="true" aria-label="Search all chats">
      <div className="ph-search-top">
        <input
          type="search"
          className="ph-search-input"
          aria-label="Search all chats"
          placeholder="Search all chats"
          value={query}
          maxLength={SEARCH_MAX_CHARS}
          autoFocus
          onChange={(event) => {
            setQuery(event.target.value);
            setClosedHit(null);
          }}
        />
        <button type="button" className="ph-search-close" onClick={onClose}>
          Cancel
        </button>
      </div>
      <div className="ph-search-body" aria-live="polite">
        {body}
      </div>
    </section>
  );
}
