import { describe, expect, it } from 'vitest';
import { INTENT_CACHE, INTENT_KEY, INTENT_TTL_MS, takeIntent, writeIntent, type IntentCaches } from './push-intent';

/** A fake Cache Storage: named caches holding responses by key. */
function fakeCaches() {
  const store = new Map<string, Map<string, Response>>();
  const log: string[] = [];
  const caches: IntentCaches = {
    async open(name) {
      const cache = store.get(name) ?? new Map<string, Response>();
      store.set(name, cache);
      return {
        async put(key, response) {
          log.push(`put ${key}`);
          cache.set(key, response);
        },
        async match(key) {
          const hit = cache.get(key);
          return hit ? hit.clone() : undefined;
        },
        async delete(key) {
          log.push(`delete ${key}`);
          return cache.delete(key);
        },
      };
    },
  };
  return { caches, store, log };
}

const NOW = 1_800_000_000_000;

describe('the intent cache (iOS: a frozen PWA misses the message)', () => {
  it('writes the intent under the fixed key and reads it back once', async () => {
    const { caches, store, log } = fakeCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', paneId: 'p1', at: NOW });
    expect(store.has(INTENT_CACHE)).toBe(true);
    expect(log).toEqual([`put ${INTENT_KEY}`]);
    expect(await takeIntent(caches, NOW + 2_000)).toEqual({ itemId: 'a7Kp2xQ9', paneId: 'p1', view: undefined, at: NOW });
    expect(await takeIntent(caches, NOW + 2_001)).toBeNull();
  });

  it('forgets an intent older than sixty seconds, and still clears it', async () => {
    const { caches, store } = fakeCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', at: NOW });
    expect(await takeIntent(caches, NOW + INTENT_TTL_MS + 1)).toBeNull();
    expect(store.get(INTENT_CACHE)!.size).toBe(0);
  });

  it('the intent is written before anything else (before the window is focused or opened)', async () => {
    // The service worker awaits writeIntent first; this pins the contract the worker relies on.
    const { caches, log } = fakeCaches();
    const order: string[] = [];
    await writeIntent(caches, { paneId: 'p1', at: NOW });
    order.push(...log, 'focus');
    expect(order).toEqual([`put ${INTENT_KEY}`, 'focus']);
  });

  it('an intent that opens nothing, or a broken value, is dropped', async () => {
    const { caches } = fakeCaches();
    await writeIntent(caches, { at: NOW });
    expect(await takeIntent(caches, NOW)).toBeNull();
    const cache = await caches.open(INTENT_CACHE);
    await cache.put(INTENT_KEY, new Response('not json'));
    expect(await takeIntent(caches, NOW)).toBeNull();
    await cache.put(INTENT_KEY, new Response(JSON.stringify({ itemId: 'x' })));
    expect(await takeIntent(caches, NOW)).toBeNull();
  });

  it('keeps the question-list intent', async () => {
    const { caches } = fakeCaches();
    await writeIntent(caches, { view: 'questions', at: NOW });
    expect(await takeIntent(caches, NOW)).toMatchObject({ view: 'questions' });
  });
});
