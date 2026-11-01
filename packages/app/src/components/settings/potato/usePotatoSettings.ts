// The Potato Bot settings (`<app data>/potato/settings.json`), read once and shared by every row and sub-page that shows
// them. Moved out of sections/PotatoSection.tsx so the sub-pages can use the same store without importing the section.
import { useCallback, useEffect, useSyncExternalStore } from 'react';
import { plainError, type PotatoSettings, type PotatoTransport } from '../../../lib/potato';

interface Store {
  value: PotatoSettings | null;
  failed: string | null;
  listeners: Set<() => void>;
}
const stores = new WeakMap<PotatoTransport, Store>();

function storeOf(transport: PotatoTransport): Store {
  let store = stores.get(transport);
  if (!store) {
    store = { value: null, failed: null, listeners: new Set() };
    stores.set(transport, store);
  }
  return store;
}

export function usePotatoSettings(transport: PotatoTransport) {
  const store = storeOf(transport);
  const value = useSyncExternalStore(
    (listener) => {
      store.listeners.add(listener);
      return () => store.listeners.delete(listener);
    },
    () => store.value,
  );
  const tell = () => store.listeners.forEach((l) => l());
  useEffect(() => {
    let live = true;
    transport
      .settingsGet()
      .then((found) => {
        if (!live) return;
        store.value = found;
        store.failed = null;
        tell();
      })
      .catch((error) => {
        if (!live) return;
        store.failed = plainError(error, 'Could not read the Potato Bot settings.');
        tell();
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [transport]);
  const save = useCallback(
    async (change: Partial<PotatoSettings>) => {
      // Read again each time: the Voice rows save the same settings file, so a cached copy may be stale.
      const base = await transport.settingsGet();
      const saved = await transport.settingsSet({ ...base, ...change });
      store.value = saved;
      store.failed = null;
      store.listeners.forEach((l) => l());
      return saved;
    },
    [store, transport],
  );
  return { settings: value, failed: store.failed, save };
}
