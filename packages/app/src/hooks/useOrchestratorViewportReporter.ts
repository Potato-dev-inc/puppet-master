import { useEffect } from 'react';
import type { BridgeClient } from '../lib/bridge';
import {
  isKeyboardLikelyOpen,
  measureLayoutWidth,
  readStableLayoutWidthPx,
} from '../lib/layout-viewport';

/** Mobile PWA: report orchestrator viewport so desktop sizes the shared PTY. */
export function useOrchestratorViewportReporter(
  bridge: BridgeClient | null,
  active: boolean,
): void {
  useEffect(() => {
    if (!bridge || typeof window === 'undefined') return;
    if (typeof bridge.postOrchestratorViewport !== 'function') return;

    const viewport = window.visualViewport;
    if (!viewport) return;

    let cancelled = false;

    const post = (isActive: boolean): void => {
      if (cancelled) return;
      const width = isKeyboardLikelyOpen()
        ? readStableLayoutWidthPx()
        : measureLayoutWidth();
      void bridge.postOrchestratorViewport({
        width,
        height: viewport.height,
        active: isActive,
      }).catch(() => {
        /* bridge may be briefly unavailable */
      });
    };

    const sync = (): void => {
      post(active);
    };

    sync();
    viewport.addEventListener('resize', sync);
    viewport.addEventListener('scroll', sync);
    window.addEventListener('orientationchange', sync);

    return () => {
      cancelled = true;
      viewport.removeEventListener('resize', sync);
      viewport.removeEventListener('scroll', sync);
      window.removeEventListener('orientationchange', sync);
      post(false);
    };
  }, [bridge, active]);
}
