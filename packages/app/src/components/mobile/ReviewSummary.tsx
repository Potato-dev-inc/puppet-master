// Review Desk on the phone (SPEC n33 3.9): a read-only card under the Night summary on the phone home.
// It shows `4 to review · 1 high risk`, then up to five rows (mark, title, agent) from `GET /review-desk`.
// The footer says Keep and Throw away are on the computer. There are no buttons here. Hidden when nothing
// is waiting, and it keeps the last list it read when a read fails.
import { useEffect, useState } from 'react';
import { reviewGlyph, reviewSummaryLine, type PhoneReviewView, type ReviewPhoneApi } from '../../lib/mobile-review';

/** Rows shown on the phone; the rest of the list stays on the computer. */
export const REVIEW_ROWS = 5;
/** The read repeats this often while the card is up (a copy or a turn can finish on the PC at any time). */
const REFRESH_MS = 10_000;
const COMPUTER_ONLY = 'Keep and Throw away are on your computer.';

export function ReviewSummary({
  api,
  refreshKey = 0,
}: {
  api: ReviewPhoneApi | null;
  /** Bumped when the stream resynced: the card reads again at once. */
  refreshKey?: number;
}) {
  const [view, setView] = useState<PhoneReviewView | null>(null);

  useEffect(() => {
    if (!api) return;
    let live = true;
    const load = () => {
      api
        .read()
        .then((next) => {
          if (live) setView(next);
        })
        // The card is optional: a refused or failed read keeps what is shown.
        .catch(() => undefined);
    };
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [api, refreshKey]);

  if (!view || view.count === 0) return null;
  const rows = view.items.slice(0, REVIEW_ROWS);

  return (
    <div className="ph-night" data-review-count={view.count}>
      <section aria-label="Review Desk" className="ph-cards">
        <div className="ph-card">
          <div className="ph-card-cap">
            <span>Review</span>
          </div>
          <p className="ph-card-q">{reviewSummaryLine(view)}</p>
        </div>
      </section>
      {rows.length > 0 && (
        <section aria-label="Review Desk items">
          <ul className="ph-group">
            {rows.map((item, index) => (
              <li key={`${index}:${item.title}`} data-level={item.levelWord}>
                <div className="ph-row">
                  <span className="ph-glyph" aria-hidden="true">
                    {reviewGlyph(item.levelWord)}
                  </span>
                  <span className="ph-txt">
                    <span className="ph-t">{item.title}</span>
                    <span className="ph-s">{item.agentLabel}</span>
                  </span>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
      <p className="ph-foot">{COMPUTER_ONLY}</p>
    </div>
  );
}
