// The number on the sidebar's "Review" button: the cheap `review_desk_count` command, read once and again on
// the same events the desk refreshes on. Off (or no desktop shell): 0, so the button shows no number.
import { useEffect, useState } from 'react';
import { onReviewDeskRefresh, reviewDeskCount } from '../../../lib/review-desk';

export function useReviewCount(enabled: boolean): number {
  const [count, setCount] = useState(0);
  useEffect(() => {
    if (!enabled) {
      setCount(0);
      return;
    }
    let live = true;
    const load = () => {
      reviewDeskCount().then(
        (n) => {
          if (live) setCount(Number.isFinite(n) && n > 0 ? Math.floor(n) : 0);
        },
        () => {
          if (live) setCount(0);
        },
      );
    };
    load();
    const off = onReviewDeskRefresh(load);
    return () => {
      live = false;
      off();
    };
  }, [enabled]);
  return count;
}
