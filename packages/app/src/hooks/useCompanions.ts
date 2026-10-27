// The desktop's companion list (SPEC-1028 section 8). Read once through the Tauri transport, then kept current by
// the `companion://changed` event. `companions` is null when there is no companion backend (a browser, an old
// PC): the sidebar then shows no companion rows at all.
import { useCallback, useEffect, useState } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { tauriCompanionTransport, type CompanionTransport, type CompanionView, type MemoryView } from '../lib/companion';
import { isTauri } from '../lib/cockpit-api';

export interface CompanionsState {
  companions: CompanionView[] | null;
  /** Read the list again (after a command, in case the event is late). */
  refresh: () => void;
}

export function useCompanions(transport: CompanionTransport = tauriCompanionTransport): CompanionsState {
  const [companions, setCompanions] = useState<CompanionView[] | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    // No desktop shell (a browser, the tests' harness): there is no list to read, so nothing is asked and no state moves.
    if (transport === tauriCompanionTransport && !isTauri()) return;
    let live = true;
    let unlisten: UnlistenFn | undefined;
    transport
      .list()
      .then((list) => live && setCompanions(list))
      .catch(() => live && setCompanions(null));
    transport
      .onChanged?.((next) => live && setCompanions(next))
      .then((fn) => {
        if (live) unlisten = fn;
        else fn();
      })
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, [transport, tick]);

  const refresh = useCallback(() => setTick((n) => n + 1), []);
  return { companions, refresh };
}

/**
 * One companion's memory (the three files, pending items, outside changes). Read again whenever `key` changes, so a
 * state change or a new pending item shows up. `setMemory` takes the view a command returns.
 */
export function useCompanionMemory(transport: CompanionTransport, id: string | null, key: string) {
  const [memory, setMemory] = useState<MemoryView | null>(null);
  useEffect(() => {
    if (!id) return;
    let live = true;
    transport
      .memory(id)
      .then((view) => live && setMemory(view))
      .catch(() => live && setMemory(null));
    return () => {
      live = false;
    };
  }, [transport, id, key]);
  return { memory, setMemory };
}
