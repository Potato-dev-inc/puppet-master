import { NudgeScheduler } from './nudge-scheduler';

/**
 * The desktop nudge: after any button, menu or palette action, a window focus or a reclaim, the
 * open terminal is remounted from the PTY's screen (the same steps as selecting another pane and
 * coming back: new xterm, fit, screen replay at the PTY size, live). It never resizes the PTY, so
 * Claude does not reprint. Typing and the jump to bottom do not nudge. One scheduler per window.
 */
type Listener = (skipIfScrolledUp: boolean) => void;

const listeners = new Set<Listener>();
const scheduler = new NudgeScheduler((skip) => {
  for (const listener of [...listeners]) listener(skip);
});

/** Ask for a nudge (coalesced and rate limited). `skipIfScrolledUp`: not while the user reads history. */
export function nudgeDesktop(skipIfScrolledUp = true): void {
  scheduler.request(skipIfScrolledUp);
}

/** The terminal that remounts on a nudge subscribes here. */
export function onDesktopNudge(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Elements whose click counts as an action (the terminal itself and typing never do). */
const ACTION_SELECTOR = 'button, a[href], [role="button"], [role="tab"], [role="menuitem"], [role="option"]';
const EXCLUDED_SELECTOR = '.terminal-host, [data-no-nudge], .pm-terminal-control-button--jump';

/** True when a click on `target` is a button/menu action that should nudge. */
export function isNudgeAction(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return !!target.closest(ACTION_SELECTOR) && !target.closest(EXCLUDED_SELECTOR);
}

/** `useNudgeOnAction()`: one helper for the cockpit and pop-outs. */
export function useNudgeOnAction(): { nudge: typeof nudgeDesktop; onClickCapture: (event: { target: EventTarget | null }) => void } {
  return {
    nudge: nudgeDesktop,
    onClickCapture: (event) => {
      if (isNudgeAction(event.target)) nudgeDesktop(true);
    },
  };
}
