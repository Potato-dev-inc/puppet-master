// Turn summaries (latest per pane) and the "While you were away" digest for the cockpit window.
// Backend data only (Rust: turn_summary): events `turn://summary` and `digest://ready`, plus
// the commands `turn_summaries`, `away_digest_pending` and `presence_cockpit_focus`.
import { useCallback, useEffect, useRef, useState } from 'react';
import { cockpitApi, type Digest, type TurnSummary } from '../lib/cockpit-api';

type Api = Pick<
  typeof cockpitApi,
  | 'turnSummaries'
  | 'onTurnSummary'
  | 'onDigestReady'
  | 'awayDigestPending'
  | 'awayDigestDismiss'
  | 'presenceCockpitFocus'
>;

export interface AwayState {
  /** Latest summary per pane id. */
  lastTurn: Map<string, TurnSummary>;
  digest: Digest | null;
  dismissDigest: () => void;
}

export function useAway(paneIds: string[], api: Api = cockpitApi): AwayState {
  const [lastTurn, setLastTurn] = useState<Map<string, TurnSummary>>(new Map());
  const [digest, setDigest] = useState<Digest | null>(null);
  const requested = useRef(new Set<string>());

  const put = useCallback((s: TurnSummary) => {
    setLastTurn((old) => {
      const prev = old.get(s.paneId);
      if (prev && (prev.turn > s.turn || (prev.turn === s.turn && prev.endedAtMs > s.endedAtMs))) return old;
      return new Map(old).set(s.paneId, s);
    });
  }, []);

  // The newest summary of every pane we have not asked about yet (also fills in after a restart).
  const key = paneIds.join(',');
  useEffect(() => {
    for (const id of paneIds) {
      if (requested.current.has(id)) continue;
      requested.current.add(id);
      api
        .turnSummaries(id)
        .then((items) => {
          const newest = items[items.length - 1];
          if (newest) put(newest);
        })
        .catch(() => requested.current.delete(id));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, api, put]);

  useEffect(() => {
    let disposed = false;
    const offs: (() => void)[] = [];
    const keep = (p: Promise<() => void>) => void p.then((off) => (disposed ? off() : offs.push(off))).catch(() => {});
    keep(api.onTurnSummary(put));
    keep(api.onDigestReady((d) => setDigest(d)));
    // A digest may be waiting from before this window opened.
    api.awayDigestPending().then((d) => !disposed && d && setDigest(d)).catch(() => {});
    // Opening the cockpit counts as being at the desk; later focus follows the window.
    void api.presenceCockpitFocus(true).catch(() => {});
    const onFocus = () => void api.presenceCockpitFocus(true).catch(() => {});
    const onBlur = () => void api.presenceCockpitFocus(false).catch(() => {});
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    return () => {
      disposed = true;
      offs.forEach((off) => off());
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    };
  }, [api, put]);

  const dismissDigest = useCallback(() => {
    setDigest(null);
    void api.awayDigestDismiss().catch(() => {});
  }, [api]);

  return { lastTurn, digest, dismissDigest };
}
