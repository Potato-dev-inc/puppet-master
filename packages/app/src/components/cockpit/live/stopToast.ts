// The toast after a Live Room ends: one plain sentence per reason (the `stopped` word of a `LiveStatus`).
// The room's link is never part of it. The only time shown is when the room was meant to end.
import { useEffect, useRef } from 'react';
import type { LiveStatus } from '../../../lib/live-room';
import { clockText } from './GoLiveSheet';

/** The sentence for a stop reason, or null when the person should not be told (the app is closing). */
export function stopSentence(reason: string | null, endsAtMs: number): string | null {
  switch (reason) {
    case 'expired':
      return 'Live Room ended: time is up.';
    case 'idle':
      return 'Live Room stopped: nothing happened for 20 minutes.';
    case 'all_panes_closed':
      return 'Live Room stopped: its panes are closed.';
    case 'relay_lost':
      return endsAtMs > 0 ? `Live Room stopped here. The link ends by itself at ${clockText(endsAtMs)}.` : 'Live Room stopped here. The link ends by itself.';
    case 'relay_ended':
      return 'Live Room ended by the relay.';
    case 'clock_jump':
      return 'Live Room stopped while this PC was asleep.';
    case 'app_quit':
      return null;
    default:
      // `owner`, and any word a newer app adds: the plain sentence is still true.
      return 'Live Room stopped.';
  }
}

/** Tells the person when a room that was live is not any more. A stopped status seen at start (no room before) says nothing. */
export function useLiveStopToast(status: LiveStatus | null, notify: (text: string) => void): void {
  const wasLive = useRef(false);
  const endsAt = useRef(0);
  useEffect(() => {
    if (!status) return;
    if (status.live) {
      wasLive.current = true;
      endsAt.current = status.endsAtMs;
      return;
    }
    if (!wasLive.current) return;
    wasLive.current = false;
    const text = stopSentence(status.stopped, status.endsAtMs > 0 ? status.endsAtMs : endsAt.current);
    if (text) notify(text);
  }, [status, notify]);
}
