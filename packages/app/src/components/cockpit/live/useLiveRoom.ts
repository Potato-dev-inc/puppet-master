// The Live Room status, kept fresh: one read on mount, then every `live://state` event. Outside the desktop
// shell the reads fail and the status stays null, so the LIVE bar and chips simply do not show.
import { useCallback, useEffect, useState } from 'react';
import { liveRoomApi, onLiveState, type LiveRoomApi, type LiveStatus } from '../../../lib/live-room';

export type LiveSubscribe = (cb: (status: LiveStatus) => void) => Promise<() => void>;

/** A readable sentence for an error from the app or from a test double. */
export const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

/** Copies the full link to the clipboard. The link is fetched here and never stored in React state. */
export async function copyRoomLink(api: Pick<LiveRoomApi, 'copyLink'>): Promise<void> {
  const link = await api.copyLink();
  await navigator.clipboard.writeText(link);
}

export function useLiveRoom({
  api = liveRoomApi,
  subscribe = onLiveState,
  enabled = true,
}: {
  api?: Pick<LiveRoomApi, 'status'>;
  subscribe?: LiveSubscribe;
  enabled?: boolean;
} = {}) {
  const [status, setStatus] = useState<LiveStatus | null>(null);

  const refresh = useCallback(() => {
    if (!enabled) return;
    // A failed read keeps the last status. Hiding the LIVE bar on a glitch would hide a room that still runs.
    api
      .status()
      .then(setStatus)
      .catch(() => {});
  }, [api, enabled]);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let unlisten: (() => void) | undefined;
    refresh();
    subscribe((next) => {
      if (live) setStatus(next);
    })
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      })
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, [enabled, refresh, subscribe]);

  return { status, refresh };
}
