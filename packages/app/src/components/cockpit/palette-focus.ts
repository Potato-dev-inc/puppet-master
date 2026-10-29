// Focus in the command palette (SPEC n33 3.6 (3)). While Focus is off: "Focus 25 minutes", "Focus 45 minutes",
// "Focus 90 minutes", the length the person last used first. While it is on: "End Focus" (with the time left) and
// "Add 15 minutes to Focus", then the three lengths again (picking one starts over from now). All actions go
// through `lib/focus.ts`.
import { createElement, type ReactNode } from 'react';
import { FOCUS_PRESETS, focusEnd, focusExtend, focusStart, leftText, type FocusView } from '../../lib/focus';
import '../../styles/focus-pill.css';

/** The length the person last started from this PC. A convenience only: blocked storage means the default order. */
const LAST_KEY = 'pm.focus.lastMinutes';

/** The palette row shape (the same fields as the palette's own rows). */
export interface FocusPaletteItem {
  id: string;
  group: 'Actions';
  title: string;
  desc?: string;
  keywords?: string;
  icon: ReactNode;
  run: () => void;
}

/** Remembers the length the person started with, so the palette lists it first next time. */
export function rememberFocusMinutes(minutes: number): void {
  try {
    window.localStorage.setItem(LAST_KEY, String(minutes));
  } catch {
    // Storage is blocked: the palette keeps the default order.
  }
}

/** The length started last, or null when none was saved or it is not one of the presets. */
export function lastFocusMinutes(): number | null {
  try {
    const n = Number(window.localStorage.getItem(LAST_KEY));
    return (FOCUS_PRESETS as readonly number[]).includes(n) ? n : null;
  } catch {
    return null;
  }
}

/** The presets in palette order: the last used length first, then the rest in their usual order. */
export function presetOrder(last: number | null): number[] {
  const first = last !== null && (FOCUS_PRESETS as readonly number[]).includes(last) ? [last] : [];
  return [...first, ...FOCUS_PRESETS.filter((m) => !first.includes(m))];
}

/** Focus is on right now: the view says so and its end is still ahead of `now`. */
export function focusIsOn(view: FocusView | null, now: number): boolean {
  return view?.on === true && view.endsAtMs !== null && view.endsAtMs > now;
}

/** Runs one Focus call; a refusal or a lost connection shows one plain line instead. */
function attempt(call: () => Promise<unknown>, failed: string, notify: (text: string) => void): void {
  Promise.resolve()
    .then(call)
    .catch(() => notify(failed));
}

/** The Focus rows for the palette. A view that has not been read yet counts as off. */
export function focusPaletteItems(view: FocusView | null, now: number, last: number | null, notify: (text: string) => void): FocusPaletteItem[] {
  const out: FocusPaletteItem[] = [];
  const on = focusIsOn(view, now);
  if (on && view?.endsAtMs != null) {
    out.push({
      id: 'focus:end',
      group: 'Actions',
      title: 'End Focus',
      desc: leftText(view.endsAtMs, now),
      keywords: 'focus stop finish early',
      icon: createElement('span', { className: 'ck-focus-dot', 'aria-hidden': 'true' }),
      run: () => attempt(focusEnd, "Couldn't end Focus. Try again.", notify),
    });
    out.push({
      id: 'focus:extend',
      group: 'Actions',
      title: 'Add 15 minutes to Focus',
      keywords: 'focus more time extend longer',
      icon: createElement('span', { className: 'ck-focus-dot', 'aria-hidden': 'true' }),
      run: () => attempt(focusExtend, "Couldn't add time to Focus. Try again.", notify),
    });
  }
  for (const minutes of presetOrder(last)) {
    out.push({
      id: `focus:start:${minutes}`,
      group: 'Actions',
      title: `Focus ${minutes} minutes`,
      desc: on ? 'Starts again from now' : undefined,
      keywords: 'focus deep work heads down',
      icon: createElement('span', { className: 'ck-focus-dot', 'aria-hidden': 'true' }),
      run: () => {
        rememberFocusMinutes(minutes);
        attempt(() => focusStart(minutes), "Couldn't start Focus. Try again.", notify);
      },
    });
  }
  return out;
}
