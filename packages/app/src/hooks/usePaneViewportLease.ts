import { useCallback, useEffect, useRef } from 'react';
import type { BridgeClient } from '../lib/bridge';

/**
 * This page's id for the bridge: several phones typing into one pane each hold their own lease and
 * the pane takes the smallest of their sizes, instead of them taking turns resizing it.
 */
export const VIEWER_ID = `v-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

/** Renew well inside the bridge's 15 s lease (`pty::viewer_size::LEASE_TTL_MS`). */
const RENEW_MS = 5000;
/** Hidden this long before the size is given up: a glance at the app switcher or notification
 * shade (or a host that flips visibility) must not bounce the agent between sizes. */
const HIDDEN_RELEASE_MS = 3000;

interface LeaseHandle {
  /** Phone input: take the size with the current grid unless this phone already holds it. */
  claimNow(): void;
  /** The fitting grid changed: re-claim with it if this phone holds the size. */
  gridChanged(): void;
}

/**
 * Mobile PWA: the phone sizes `paneId` to its `grid` only when the user types into it (`claimNow`).
 * Viewing never resizes the pane. While the phone holds the size it renews the lease; the bridge
 * keeps the size after the phone leaves, hides or goes quiet (sticky). The desktop window takes it
 * back, which the renewal notices (`held: false`) and then stops renewing.
 */
export function usePaneViewportLease(
  bridge: BridgeClient | null,
  paneId: string | null,
  grid: { cols: number; rows: number } | null,
): () => void {
  const gridRef = useRef(grid);
  gridRef.current = grid;
  const gridKey = grid ? `${grid.cols}x${grid.rows}` : '';
  const leaseRef = useRef<LeaseHandle | null>(null);

  // One lease per pane shown: renewed on a timer while held, released when the phone leaves it.
  useEffect(() => {
    if (!bridge || !paneId) return;
    let holding = false;
    let heldKey = '';
    /** Bumped on every release or claim so a late renewal answer cannot undo a newer claim. */
    let epoch = 0;
    let hiddenTimer = 0;

    const claim = () => {
      const g = gridRef.current;
      if (!g) return;
      window.clearTimeout(hiddenTimer);
      epoch += 1;
      holding = true;
      heldKey = `${g.cols}x${g.rows}`;
      void bridge.setPaneViewport(paneId, { ...g, active: true, viewer: VIEWER_ID }).catch(() => {});
    };
    const renew = () => {
      if (!holding) return;
      const sentEpoch = epoch;
      const g = gridRef.current ?? { cols: 0, rows: 0 };
      void bridge
        .setPaneViewport(paneId, { ...g, active: true, viewer: VIEWER_ID, renew_only: true })
        .then((res) => {
          // The desktop took the size back (or the lease lapsed): stop renewing until the next input.
          if (res?.held === false && sentEpoch === epoch) holding = false;
        })
        .catch(() => {});
    };
    const release = () => {
      window.clearTimeout(hiddenTimer);
      if (!holding) return;
      holding = false;
      epoch += 1;
      void bridge.setPaneViewport(paneId, { cols: 0, rows: 0, active: false, viewer: VIEWER_ID }).catch(() => {});
    };
    const onVisibility = () => {
      window.clearTimeout(hiddenTimer);
      if (document.visibilityState === 'visible') return;
      hiddenTimer = window.setTimeout(() => {
        if (document.visibilityState !== 'visible') release();
      }, HIDDEN_RELEASE_MS);
    };

    leaseRef.current = {
      claimNow: () => {
        const g = gridRef.current;
        if (!g) return;
        // Typing with the size already held costs no request.
        if (holding && heldKey === `${g.cols}x${g.rows}`) return;
        claim();
      },
      gridChanged: () => {
        const g = gridRef.current;
        if (!holding || !g || heldKey === `${g.cols}x${g.rows}`) return;
        claim();
      },
    };
    const timer = window.setInterval(renew, RENEW_MS);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('pagehide', release);
    return () => {
      leaseRef.current = null;
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('pagehide', release);
      release();
    };
  }, [bridge, paneId]);

  // A new fitting grid (first measure, rotation, keyboard) is re-claimed at once while held, without
  // a release in between, so the size does not flash back.
  useEffect(() => {
    if (gridKey) leaseRef.current?.gridChanged();
  }, [gridKey, paneId]);

  return useCallback(() => leaseRef.current?.claimNow(), []);
}
