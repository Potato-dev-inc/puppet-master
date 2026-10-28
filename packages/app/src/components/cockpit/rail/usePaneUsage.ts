// The helpers' usage for the Helpers card. Read again whenever `stamp` changes: the rail builds it from the
// pane's and its helpers' statuses, which change when a turn ends, so the numbers follow the turns.
import { useEffect, useState } from 'react';
import { paneUsage, type PaneUsage } from '../../../lib/pane-usage';

export function usePaneUsage(paneId: string, enabled: boolean, stamp: string): PaneUsage | null {
  const [usage, setUsage] = useState<PaneUsage | null>(null);
  useEffect(() => {
    if (!enabled) {
      setUsage(null);
      return;
    }
    let live = true;
    paneUsage(paneId)
      .then((next) => {
        if (live) setUsage(next);
      })
      .catch(() => {
        if (live) setUsage(null);
      });
    return () => {
      live = false;
    };
  }, [paneId, enabled, stamp]);
  return usage;
}
