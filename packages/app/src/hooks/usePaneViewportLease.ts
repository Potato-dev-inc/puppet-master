import { useEffect, useRef } from 'react';
import type { BridgeClient } from '../lib/bridge';

/**
 * This page's id for the bridge: several phones viewing one pane each hold their own lease and the
 * pane takes the smallest of their sizes, instead of them taking turns resizing it.
 */
const VIEWER_ID = `v-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

/** Renew well inside the bridge's 15 s lease (`pty::viewer_size::LEASE_TTL_MS`). */
const RENEW_MS = 5000;
/** Hidden this long before the pane is given back: a glance at the app switcher or notification
 * shade (or a host that flips visibility) must not bounce the agent between sizes. */
const HIDDEN_RELEASE_MS = 3000;

/**
 * Mobile PWA: while the phone shows `paneId` and the app is visible, size that pane to the phone's
 * `grid` so the agent redraws for the phone. Leaving the tab, hiding the app or unmounting gives
 * the pane back to its desktop size (and a phone that just vanishes lets the lease lapse).
 */
export function usePaneViewportLease(
  bridge: BridgeClient | null,
  paneId: string | null,
  grid: { cols: number; rows: number } | null,
): void {
  const gridRef = useRef(grid);
  gridRef.current = grid;
  const gridKey = grid ? `${grid.cols}x${grid.rows}` : '';
  const claimRef = useRef<(() => void) | null>(null);

  // One lease per pane shown: renewed on a timer, released when the phone leaves it.
  useEffect(() => {
    if (!bridge || !paneId) return;
    let holding = false;
    let hiddenTimer = 0;
    const claim = () => {
      const g = gridRef.current;
      if (!g || document.visibilityState !== 'visible') return;
      window.clearTimeout(hiddenTimer);
      holding = true;
      void bridge.setPaneViewport(paneId, { ...g, active: true, viewer: VIEWER_ID }).catch(() => {});
    };
    const release = () => {
      if (!holding) return;
      holding = false;
      void bridge.setPaneViewport(paneId, { cols: 0, rows: 0, active: false, viewer: VIEWER_ID }).catch(() => {});
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') {
        claim();
        return;
      }
      window.clearTimeout(hiddenTimer);
      hiddenTimer = window.setTimeout(() => {
        if (document.visibilityState !== 'visible') release();
      }, HIDDEN_RELEASE_MS);
    };
    claimRef.current = claim;
    const timer = window.setInterval(claim, RENEW_MS);
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('pagehide', release);
    return () => {
      claimRef.current = null;
      window.clearInterval(timer);
      window.clearTimeout(hiddenTimer);
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('pagehide', release);
      release();
    };
  }, [bridge, paneId]);

  // A new fitting grid (first measure, rotation, keyboard) is claimed at once, without a release
  // in between, so the desktop size does not flash back.
  useEffect(() => {
    if (gridKey) claimRef.current?.();
  }, [gridKey, paneId]);
}
