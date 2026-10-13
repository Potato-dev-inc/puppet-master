import { useEffect } from 'react';
import { reportPresence } from '../lib/push-client';
import { isOpenPaneMessage, isOpenViewMessage, paneFromSearch, viewFromSearch } from '../pwa/push-core';

const HEARTBEAT_MS = 20_000;

/**
 * Push plumbing of the phone PWA:
 * - while the app is on screen, tell the PC every 20 s (and on every show/hide) so it does not
 *   notify a phone that is already looking;
 * - open the pane a tapped notification points at (service worker message, or `?pane=` on a
 *   fresh launch);
 * - open the question list a tapped question notification points at (`?view=questions` on a
 *   fresh launch, or the view message from the service worker).
 */
export function usePushPresence(
  bridgeUrl: string | null,
  onOpenPane: (paneId: string) => void,
  onOpenQuestions?: () => void,
): void {
  useEffect(() => {
    const launchPane = paneFromSearch(window.location.search);
    const launchView = viewFromSearch(window.location.search);
    if (launchPane) {
      onOpenPane(launchPane);
      window.history.replaceState(null, '', window.location.pathname);
    } else if (launchView) {
      onOpenQuestions?.();
      window.history.replaceState(null, '', window.location.pathname);
    }
    const onMessage = (event: MessageEvent) => {
      if (isOpenPaneMessage(event.data)) onOpenPane(event.data.pane_id);
      else if (isOpenViewMessage(event.data)) onOpenQuestions?.();
    };
    navigator.serviceWorker?.addEventListener('message', onMessage);
    return () => navigator.serviceWorker?.removeEventListener('message', onMessage);
    // Launch handling must run once; onOpenPane is a stable state setter.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!bridgeUrl || typeof Notification === 'undefined' || Notification.permission !== 'granted') return;
    const report = () => void reportPresence(bridgeUrl, document.visibilityState === 'visible');
    report();
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') report();
    }, HEARTBEAT_MS);
    document.addEventListener('visibilitychange', report);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', report);
    };
  }, [bridgeUrl]);
}
