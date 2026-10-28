// The Focus state the Inbox note reads (SPEC n33 3.6): the view on mount, every `focus://changed`, and a
// clock that ticks every 30 s while Focus is on (for "31 min left"). A failed read leaves the view empty,
// so the note draws nothing instead of a wrong state.
import { useEffect, useState } from 'react';
import { focusState, onFocusChanged, type FocusView } from '../lib/focus';

/** How often the "min left" count refreshes while Focus is on. */
export const FOCUS_TICK_MS = 30_000;

export interface FocusNow {
  /** The Focus view, or null until the first read (or when there is no desktop shell). */
  view: FocusView | null;
  /** The clock for the countdown. */
  now: number;
  /** Replace the view with a command's answer (the event brings the same view a moment later). */
  set: (view: FocusView) => void;
}

export function useFocus(): FocusNow {
  const [view, setView] = useState<FocusView | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const on = view?.on === true;

  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    const first = async () => {
      try {
        const v = await focusState();
        if (alive) setView(v);
      } catch {
        // No desktop shell (a browser preview, a test) or the command failed: no note.
      }
    };
    void first();
    onFocusChanged((v) => {
      if (alive) setView(v);
    })
      .then((off) => {
        if (alive) stop = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      stop?.();
    };
  }, []);

  useEffect(() => {
    if (!on) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), FOCUS_TICK_MS);
    return () => window.clearInterval(timer);
  }, [on]);

  return { view, now, set: setView };
}
