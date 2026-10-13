// Plain-language activity line per pane ("Running npm test"). Backend: `pane_activities` command
// for the current phrases and the `pty://activity` event for changes (see src-tauri pane_activity.rs).
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type ActivityKind = 'idle' | 'working' | 'waiting' | 'error';

export interface PaneActivity {
  kind: ActivityKind;
  text: string;
}

export type ActivityMap = Record<string, PaneActivity>;

const KIND_FOR_STATUS: Record<string, ActivityKind> = {
  running: 'working',
  waiting_input: 'waiting',
  idle: 'idle',
  error: 'error',
};

/**
 * The phrase to show for a pane, or null. A phrase computed for another status (the pane just
 * changed state and the backend has not caught up, at most about a second) is hidden so the line
 * never contradicts the status glyph next to it.
 */
export function activityText(status: string, activity: PaneActivity | undefined): string | null {
  if (!activity || !activity.text) return null;
  const expected = KIND_FOR_STATUS[status];
  return expected && expected !== activity.kind ? null : activity.text;
}

const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function usePaneActivity(): ActivityMap {
  const [map, setMap] = useState<ActivityMap>({});
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let off: (() => void) | undefined;
    invoke<ActivityMap>('pane_activities')
      .then((initial) => !disposed && setMap((cur) => ({ ...initial, ...cur })))
      .catch(() => {});
    listen<{ pane_id: string; activity: PaneActivity }>('pty://activity', (e) =>
      setMap((cur) => ({ ...cur, [e.payload.pane_id]: e.payload.activity })),
    )
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  return map;
}
