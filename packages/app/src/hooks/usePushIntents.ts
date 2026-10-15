import { useEffect, useRef } from 'react';
import { reportPresence } from '../lib/push-client';
import {
  isOpenItemMessage,
  isOpenPaneMessage,
  isOpenViewMessage,
  itemFromSearch,
  paneFromSearch,
  viewFromSearch,
} from '../pwa/push-core';
import { takeIntent, type IntentCaches } from '../pwa/push-intent';

const HEARTBEAT_MS = 20_000;

export interface PushIntentHandlers {
  /** A tapped notification points at this card (`/?item=`). The pane id, when known, is a fallback. */
  onOpenItem?: (itemId: string, paneId?: string | null) => void;
  onOpenPane: (paneId: string) => void;
  onOpenQuestions?: () => void;
}

/**
 * Push plumbing of the phone PWA:
 * - while the app is on screen, tell the PC every 20 s (and on every show/hide) so it does not
 *   notify a phone that is already looking;
 * - open what a tapped notification points at, from the three places that can carry it: the launch
 *   URL (`?item=` / `?pane=` / `?view=`), the service worker's message to a window that is open, and
 *   the intent cache the service worker wrote before it focused the app. The cache is read when the
 *   app starts AND every time it comes to the front, because an iOS PWA frozen in the background
 *   misses the message. An intent opens once.
 */
export function usePushIntents(bridgeUrl: string | null, handlers: PushIntentHandlers): void {
  const latest = useRef(handlers);
  latest.current = handlers;

  useEffect(() => {
    const open = (intent: { itemId?: string | null; paneId?: string | null; view?: 'questions' }) => {
      const h = latest.current;
      if (intent.itemId && h.onOpenItem) h.onOpenItem(intent.itemId, intent.paneId);
      else if (intent.paneId) h.onOpenPane(intent.paneId);
      else if (intent.view === 'questions') h.onOpenQuestions?.();
    };
    /** Read the cached intent and forget it; `apply` = false only clears it (the same intent already arrived another way). */
    const readCache = async (apply = true) => {
      const caches = (globalThis as { caches?: IntentCaches }).caches;
      if (!caches) return;
      try {
        const intent = await takeIntent(caches, Date.now());
        if (intent && apply) open(intent);
      } catch {
        /* Cache Storage unavailable (private window, insecure origin) */
      }
    };

    const itemId = itemFromSearch(window.location.search);
    const paneId = paneFromSearch(window.location.search);
    const view = viewFromSearch(window.location.search);
    if (itemId || paneId || view) {
      open({ itemId, paneId, view: view ?? undefined });
      window.history.replaceState(null, '', window.location.pathname);
      // The service worker wrote the same intent before opening this window: forget it, or the
      // next time the app comes to the front it opens the card a second time.
      void readCache(false);
    } else {
      void readCache();
    }

    const onMessage = (event: MessageEvent) => {
      if (isOpenItemMessage(event.data)) {
        open({ itemId: event.data.item_id, paneId: event.data.pane_id });
        void readCache(false);
      } else if (isOpenPaneMessage(event.data)) {
        open({ paneId: event.data.pane_id });
        void readCache(false);
      } else if (isOpenViewMessage(event.data)) {
        open({ view: 'questions' });
        void readCache(false);
      }
    };
    const onVisible = () => {
      if (document.visibilityState === 'visible') void readCache();
    };
    navigator.serviceWorker?.addEventListener('message', onMessage);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      navigator.serviceWorker?.removeEventListener('message', onMessage);
      document.removeEventListener('visibilitychange', onVisible);
    };
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
