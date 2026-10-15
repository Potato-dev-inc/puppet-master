// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { INTENT_KEY, writeIntent, type IntentCaches } from '../pwa/push-intent';
import { usePushIntents, type PushIntentHandlers } from './usePushIntents';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe({ handlers }: { handlers: PushIntentHandlers }) {
  usePushIntents(null, handlers);
  return null;
}

/** A fake Cache Storage shared by the test and the hook (`globalThis.caches`). */
function installCaches() {
  const entries = new Map<string, Response>();
  const caches: IntentCaches = {
    async open() {
      return {
        async put(key, response) {
          entries.set(key, response);
        },
        async match(key) {
          return entries.get(key)?.clone();
        },
        async delete(key) {
          return entries.delete(key);
        },
      };
    },
  };
  (globalThis as { caches?: IntentCaches }).caches = caches;
  return { caches, entries };
}

let host: HTMLDivElement;
let root: Root;
let sw: EventTarget;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  sw = new EventTarget();
  Object.defineProperty(navigator, 'serviceWorker', { value: sw, configurable: true });
  window.history.replaceState(null, '', '/');
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (globalThis as { caches?: unknown }).caches;
});

const handlers = () => ({ onOpenItem: vi.fn(), onOpenPane: vi.fn(), onOpenQuestions: vi.fn() });
const mount = async (h: PushIntentHandlers) => {
  await act(async () => {
    root.render(<Probe handlers={h} />);
  });
};
const flush = () => act(async () => void (await Promise.resolve()));

describe('opening what a notification points at', () => {
  it('a launch url with ?item= opens the card and clears the address', async () => {
    window.history.replaceState(null, '', '/?item=a7Kp2xQ9&pane=p1');
    const h = handlers();
    await mount(h);
    expect(h.onOpenItem).toHaveBeenCalledWith('a7Kp2xQ9', 'p1');
    expect(h.onOpenPane).not.toHaveBeenCalled();
    expect(window.location.search).toBe('');
  });

  it('?pane= opens the pane and ?view=questions the question list', async () => {
    window.history.replaceState(null, '', '/?pane=p9');
    const a = handlers();
    await mount(a);
    expect(a.onOpenPane).toHaveBeenCalledWith('p9');
    act(() => root.unmount());
    root = createRoot(host);
    window.history.replaceState(null, '', '/?view=questions');
    const b = handlers();
    await mount(b);
    expect(b.onOpenQuestions).toHaveBeenCalledTimes(1);
  });

  it('a message from the service worker opens the card of an app that is already open', async () => {
    const h = handlers();
    await mount(h);
    await act(async () => {
      sw.dispatchEvent(new MessageEvent('message', { data: { type: 'pm-open-item', item_id: 'a7Kp2xQ9', pane_id: 'p1' } }));
    });
    expect(h.onOpenItem).toHaveBeenCalledWith('a7Kp2xQ9', 'p1');
    await act(async () => {
      sw.dispatchEvent(new MessageEvent('message', { data: { type: 'pm-open-pane', pane_id: 'p2' } }));
    });
    expect(h.onOpenPane).toHaveBeenCalledWith('p2');
  });

  it('an intent the service worker cached opens when the app starts, once', async () => {
    const { caches, entries } = installCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', paneId: 'p1', at: Date.now() });
    const h = handlers();
    await mount(h);
    await flush();
    expect(h.onOpenItem).toHaveBeenCalledTimes(1);
    expect(h.onOpenItem).toHaveBeenCalledWith('a7Kp2xQ9', 'p1');
    expect(entries.has(INTENT_KEY)).toBe(false);
  });

  it('an app frozen in the background reads the cache when it comes back to the front (iOS)', async () => {
    const { caches } = installCaches();
    const h = handlers();
    await mount(h);
    await flush();
    expect(h.onOpenItem).not.toHaveBeenCalled();
    // The notification was tapped while the app was frozen: the worker wrote the intent.
    await writeIntent(caches, { itemId: 'h1ghR1sk', at: Date.now() });
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await flush();
    expect(h.onOpenItem).toHaveBeenCalledWith('h1ghR1sk', null);
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await flush();
    expect(h.onOpenItem).toHaveBeenCalledTimes(1);
  });

  it('the same tap arriving by url AND by cache opens once and leaves nothing to open again', async () => {
    const { caches, entries } = installCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', at: Date.now() });
    window.history.replaceState(null, '', '/?item=a7Kp2xQ9');
    const h = handlers();
    await mount(h);
    await flush();
    expect(h.onOpenItem).toHaveBeenCalledTimes(1);
    expect(entries.has(INTENT_KEY)).toBe(false);
  });

  it('an intent older than sixty seconds is not opened', async () => {
    const { caches } = installCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', at: Date.now() - 61_000 });
    const h = handlers();
    await mount(h);
    await flush();
    expect(h.onOpenItem).not.toHaveBeenCalled();
  });

  it('falls back to the pane when the host cannot open cards', async () => {
    const { caches } = installCaches();
    await writeIntent(caches, { itemId: 'a7Kp2xQ9', paneId: 'p1', at: Date.now() });
    const onOpenPane = vi.fn();
    await mount({ onOpenPane });
    await flush();
    expect(onOpenPane).toHaveBeenCalledWith('p1');
  });
});
