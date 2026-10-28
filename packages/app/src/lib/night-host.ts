// How the places that start a night reach the one Tonight sheet: the sidebar mounts the sheet once, and the
// palette asks it to open through this small bus (the same shape as `openStartMission` for missions).
// Also the Undo switch, which the sheet needs to know before it lets Start through.
import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

type Listener = () => void;
const listeners = new Set<Listener>();

/** Asks the sidebar to open the Tonight sheet. */
export function openTonight(): void {
  listeners.forEach((l) => l());
}

/** The sidebar's side: called each time something asks for the Tonight sheet. Returns the way to stop listening. */
export function onOpenTonight(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** The Undo switch (`checkpoints_enabled`, on by default), kept current when Settings change. Night Shift needs it. */
export function useUndoOn(): boolean {
  const [on, setOn] = useState(true);
  useEffect(() => {
    if (!inTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    const read = () =>
      import('./settings')
        .then(({ loadSettings }) => loadSettings())
        .then((s) => !disposed && setOn(s.checkpoints_enabled !== false))
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
