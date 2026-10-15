// The intent cache: what a tapped notification wants to open, written by the service worker before it
// focuses or opens the app and read once by the app. On iOS a PWA that went to the background can be
// frozen, so a postMessage sent before it thaws is lost; a value in Cache Storage is not (design 2.7).
// Pure: the cache storage is passed in, so this is tested with a fake one and compiles in the
// service-worker project too.

export const INTENT_CACHE = 'pm-intent';
export const INTENT_KEY = '/__pm_intent__';
/** An intent older than this is stale: the person opened the app some other way meanwhile. */
export const INTENT_TTL_MS = 60_000;

export interface OpenIntent {
  itemId?: string | null;
  paneId?: string | null;
  view?: 'questions';
  /** When the notification was tapped (epoch ms). */
  at: number;
}

/** The slice of Cache Storage this needs. */
export interface IntentCaches {
  open(name: string): Promise<{
    put(key: string, response: Response): Promise<void>;
    match(key: string): Promise<Response | undefined>;
    delete(key: string): Promise<boolean>;
  }>;
}

export async function writeIntent(caches: IntentCaches, intent: OpenIntent): Promise<void> {
  const cache = await caches.open(INTENT_CACHE);
  await cache.put(INTENT_KEY, new Response(JSON.stringify(intent), { headers: { 'content-type': 'application/json' } }));
}

/** Read the intent and forget it, so it opens exactly once. `null` when there is none or it is stale. */
export async function takeIntent(caches: IntentCaches, now: number): Promise<OpenIntent | null> {
  const cache = await caches.open(INTENT_CACHE);
  const hit = await cache.match(INTENT_KEY);
  if (!hit) return null;
  await cache.delete(INTENT_KEY);
  try {
    const raw = (await hit.json()) as Partial<OpenIntent>;
    if (typeof raw.at !== 'number' || now - raw.at > INTENT_TTL_MS || raw.at - now > INTENT_TTL_MS) return null;
    const itemId = typeof raw.itemId === 'string' && raw.itemId ? raw.itemId : null;
    const paneId = typeof raw.paneId === 'string' && raw.paneId ? raw.paneId : null;
    const view = raw.view === 'questions' ? 'questions' : undefined;
    if (!itemId && !paneId && !view) return null;
    return { itemId, paneId, view, at: raw.at };
  } catch {
    return null;
  }
}
