// My computers (Labs `desk_enabled`, off by default). Half 1 only shows the page shell; the engine comes later.
import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The Labs switch (`desk_enabled`). Off unless the person turns it on. */
export function deskEnabled(settings: { desk_enabled?: boolean } | null | undefined): boolean {
  return settings?.desk_enabled === true;
}

/** Whether the Labs switch is on, kept current when Settings change. */
export function useDeskEnabled(): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    if (!inTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const read = () =>
      import('./settings')
        .then(({ loadSettings }) => loadSettings())
        .then((s) => !disposed && setOn(deskEnabled(s as { desk_enabled?: boolean })))
        .catch(() => {});
    void read();
    listen('settings://changed', () => void read())
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return on;
}
