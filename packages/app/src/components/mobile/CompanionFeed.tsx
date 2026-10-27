// "While you were away" on the companion home (SPEC-1028 section 7): the feed items newer than the last time this
// phone opened the home, newest first. Five rows at first; "Show all" shows up to 50. The last-seen mark is kept in
// localStorage per companion; when that is not available (private mode, a full store) the list still shows, from the
// newest items, and nothing is stored.
import type { FeedItem } from '../../lib/companion';

export const FEED_FIRST = 5;
export const FEED_ALL = 50;
const LS_PREFIX = 'pm.mobile.companion.feedMark.';
const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

/** "8:00" in local time; an item from another day gets its short weekday in front ("Mon 8:00"). */
export function clockLabel(atMs: number, now: number = Date.now()): string {
  const at = new Date(atMs);
  const clock = `${at.getHours()}:${String(at.getMinutes()).padStart(2, '0')}`;
  return at.toDateString() === new Date(now).toDateString() ? clock : `${DAYS[at.getDay()]} ${clock}`;
}

/** The newest feed time this phone has seen for a companion (0 when none, or when storage is not there). */
export function readFeedMark(companionId: string): number {
  try {
    const value = Number(localStorage.getItem(LS_PREFIX + companionId));
    return Number.isFinite(value) && value > 0 ? value : 0;
  } catch {
    return 0;
  }
}

/** Keeps the newer of the stored mark and `atMs`. A failed write is ignored: the UI works without it. */
export function writeFeedMark(companionId: string, atMs: number): void {
  try {
    if (atMs > readFeedMark(companionId)) localStorage.setItem(LS_PREFIX + companionId, String(atMs));
  } catch {
    /* private mode or a full store */
  }
}

export function CompanionFeed({
  items,
  expanded,
  onToggle,
  now,
}: {
  items: FeedItem[];
  expanded: boolean;
  onToggle: () => void;
  /** Pins "today" for the labels (tests); defaults to the clock. */
  now?: number;
}) {
  if (items.length === 0) return null;
  const shown = items.slice(0, expanded ? FEED_ALL : FEED_FIRST);
  return (
    <section className="ph-comp-block" aria-label="While you were away">
      <h2 className="ph-comp-title">While you were away</h2>
      <ul className="ph-group">
        {shown.map((item) => (
          <li key={item.id} className="ph-comp-feed-row">
            <span className="ph-comp-time">{clockLabel(item.atMs, now)}</span>
            <span className="ph-comp-text">{item.text}</span>
          </li>
        ))}
      </ul>
      {items.length > FEED_FIRST && (
        <button type="button" className="ph-comp-more" onClick={onToggle}>
          {expanded ? 'Show less' : 'Show all'}
        </button>
      )}
    </section>
  );
}
