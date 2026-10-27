// The companions the phone shows (SPEC-1028 section 7). The list is read with `GET /companions` on start, on each
// stream resync, when the page comes back to the front, and every 5 s while the page is visible. A `companion`
// event from the bridge stream (owned by PwaApp) puts its list in at once: the PwaApp passes it in as `pushed`.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { CompanionTransport, CompanionView } from '../lib/companion';

/** How often the PC is asked for the list while the page is visible. */
export const COMPANIONS_POLL_MS = 5000;

export interface BridgeCompanions {
  companions: CompanionView[];
  /** True once the first read has settled (answered or failed), so "no companion" is not shown while a read is out. */
  loaded: boolean;
  /** Puts one companion's new view into the list (after a wake or a sleep), before the next read. */
  applyView: (view: CompanionView) => void;
}

/**
 * @param transport the phone transport (`bridgeCompanionTransport`), or null before the bridge is up.
 * @param resyncKey changes when the stream resyncs: the list is read again.
 * @param pushed the list from the latest `companion` event, or null before any event.
 */
export function useBridgeCompanions(
  transport: CompanionTransport | null,
  resyncKey: number,
  pushed: CompanionView[] | null,
): BridgeCompanions {
  const [companions, setCompanions] = useState<CompanionView[]>([]);
  const [loaded, setLoaded] = useState(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const read = useCallback(() => {
    if (!transport) return;
    // A failed read keeps the list held: a dropped link should not empty the tabs.
    transport
      .list()
      .then((list) => {
        if (!mounted.current) return;
        setCompanions(list);
        setLoaded(true);
      })
      .catch(() => {
        if (mounted.current) setLoaded(true);
      });
  }, [transport]);

  useEffect(() => {
    read();
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') read();
    }, COMPANIONS_POLL_MS);
    const onVisible = () => {
      if (document.visibilityState === 'visible') read();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [read, resyncKey]);

  useEffect(() => {
    if (!pushed) return;
    setCompanions(pushed);
    setLoaded(true);
  }, [pushed]);

  const applyView = useCallback((view: CompanionView) => {
    setCompanions((cur) => (cur.some((c) => c.id === view.id) ? cur.map((c) => (c.id === view.id ? view : c)) : [...cur, view]));
  }, []);

  return { companions, loaded, applyView };
}
