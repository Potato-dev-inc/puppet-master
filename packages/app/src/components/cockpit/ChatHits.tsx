// What the palette shows in its "Search all chats" mode: one row per hit (who said it, the words with the match
// marked, which agent and folder, when), and a read-only card for a hit whose pane is gone.
import { snippetParts, kindText, whenText, type ChatHit } from '../../lib/chatSearch';

export const SEARCH_CHATS_TITLE = 'Search all chats';
export const SEARCH_CHATS_HINT = 'Ctrl+Shift+F';

function Snippet({ hit }: { hit: ChatHit }) {
  return (
    <>
      {snippetParts(hit.snippet, hit.marks).map((part, i) =>
        part.hit ? (
          <mark key={i} className="ck-hit-mark">
            {part.text}
          </mark>
        ) : (
          <span key={i}>{part.text}</span>
        ),
      )}
    </>
  );
}

function metaText(hit: ChatHit, agentName: string, now: number): string {
  return [agentName, hit.folder, whenText(hit.atMs, now)].filter(Boolean).join(' · ');
}

export function ChatHitRows({
  hits,
  index,
  agentName,
  onHover,
  onChoose,
  now,
}: {
  hits: ChatHit[];
  index: number;
  agentName: (agentId: string) => string;
  onHover: (i: number) => void;
  onChoose: (hit: ChatHit) => void;
  now: number;
}) {
  return (
    <>
      {hits.map((hit, i) => (
        <div
          key={`${hit.paneId}:${hit.session}:${hit.itemId}`}
          role="option"
          aria-selected={i === index}
          data-active={i === index}
          className={`ck-palette-row ck-palette-row--hit${i === index ? ' is-active' : ''}`}
          onMouseMove={() => onHover(i)}
          onClick={() => onChoose(hit)}
        >
          <span className="ck-hit-who">{kindText(hit.kind)}</span>
          <span className="ck-hit-body">
            <span className="ck-hit-text">
              <Snippet hit={hit} />
            </span>
            <span className="ck-hit-meta">{metaText(hit, agentName(hit.agent), now)}</span>
          </span>
        </div>
      ))}
    </>
  );
}

/** A hit whose pane is not open any more: the snippet and where it came from, nothing to open. */
export function ChatHitDetail({ hit, agentName, onBack, now }: { hit: ChatHit; agentName: string; onBack: () => void; now: number }) {
  return (
    <div className="ck-hit-detail">
      <p className="ck-hit-detail-meta">
        {kindText(hit.kind)} · {metaText(hit, agentName, now)}
      </p>
      <p className="ck-hit-detail-text">
        <Snippet hit={hit} />
      </p>
      <p className="ck-sub">This chat is closed, so this is all that was kept.</p>
      <button type="button" className="ck-btn" onClick={onBack}>
        Back to results
      </button>
    </div>
  );
}
