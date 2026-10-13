/// <reference lib="webworker" />
// Service worker of the phone PWA (vite-plugin-pwa `injectManifest`).
//
// Part 1 reproduces what `generateSW` produced before, so installs behave the same:
// precache the build output (js, css, html, woff2), clean old caches, take over at once
// (registerType: autoUpdate), serve index.html for navigations except /bridge and the dev-info
// file, and never cache /bridge.
//
// Part 2 is push: show a notification for every push (iOS revokes subscriptions that stay
// silent), with Allow / Deny buttons when the payload carries a one-shot token, and open or
// focus the pane on tap.
import { clientsClaim } from 'workbox-core';
import { cleanupOutdatedCaches, createHandlerBoundToURL, precacheAndRoute } from 'workbox-precaching';
import { NavigationRoute, registerRoute } from 'workbox-routing';
import { NetworkOnly } from 'workbox-strategies';
import {
  OPEN_PANE_MESSAGE,
  OPEN_VIEW_MESSAGE,
  clickIntent,
  openUrl,
  parsePushPayload,
  planNotification,
} from './pwa/push-core';

declare const self: ServiceWorkerGlobalScope;

void self.skipWaiting();
clientsClaim();
cleanupOutdatedCaches();
precacheAndRoute(self.__WB_MANIFEST);
registerRoute(
  new NavigationRoute(createHandlerBoundToURL('index.html'), {
    denylist: [/^\/bridge/, /^\/__puppet_master_dev__/],
  }),
);
registerRoute(({ url }) => url.pathname.startsWith('/bridge/'), new NetworkOnly());

// --- push ---------------------------------------------------------------------------------

self.addEventListener('push', (event) => {
  const plan = planNotification(parsePushPayload(event.data?.text()), self.registration.scope);
  event.waitUntil(self.registration.showNotification(plan.title, plan.options as NotificationOptions));
});

async function answer(url: string, token: string, choice: 'allow' | 'deny'): Promise<boolean> {
  try {
    const res = await fetch(new URL(url, self.registration.scope).href, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token, choice }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

async function openPane(paneId: string | null, view?: 'questions'): Promise<void> {
  const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  const existing = windows.find((client) => 'focus' in client) as WindowClient | undefined;
  if (existing) {
    await existing.focus();
    if (view) existing.postMessage({ type: OPEN_VIEW_MESSAGE, view });
    else if (paneId) existing.postMessage({ type: OPEN_PANE_MESSAGE, pane_id: paneId });
    return;
  }
  await self.clients.openWindow(new URL(openUrl(paneId, view), self.registration.scope).href);
}

self.addEventListener('notificationclick', (event) => {
  const intent = clickIntent(event.action, event.notification.data);
  event.notification.close();
  if (intent.type === 'open') {
    event.waitUntil(openPane(intent.paneId, intent.view));
    return;
  }
  event.waitUntil(
    (async () => {
      if (await answer(intent.url, intent.token, intent.choice)) return;
      // Expired, already answered at the PC, or offline: say so and let a tap open the pane.
      await self.registration.showNotification('Could not send your answer', {
        body: 'It may already be answered. Tap to open the pane.',
        tag: event.notification.tag,
        data: { pane_id: event.notification.data?.pane_id },
      });
    })(),
  );
});
