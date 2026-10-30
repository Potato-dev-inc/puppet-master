import type { GridSize } from './owner-follow';

/**
 * Viewer/PTY grid drift. ConPTY repaints by absolute cursor position, so an xterm grid that differs
 * from the PTY's grid mid-stream garbles the screen until the person presses Reflow. The owner window
 * watches for that by itself: this is the pure part (what counts as drift, and when to act on it).
 */

/** The viewer's xterm grid differs from the PTY's grid. Unknown PTY grid or a PTY a phone holds: no drift. */
export function hasGridDrift(viewer: GridSize, pty: GridSize | null | undefined, following: boolean): boolean {
  if (following || !pty) return false;
  if (viewer.cols <= 0 || viewer.rows <= 0 || pty.cols <= 0 || pty.rows <= 0) return false;
  return viewer.cols !== pty.cols || viewer.rows !== pty.rows;
}

/** How long a drift must persist (the pane list may simply not have caught up with our own resize). */
export const DRIFT_CONFIRM_MS = 500;
/** No second correction sooner than this after one. */
export const DRIFT_COOLDOWN_MS = 2000;
/** Corrections for one and the same pair of grids: the app ignored them (not the owner), so stop asking. */
export const DRIFT_MAX_ATTEMPTS = 2;
/** Events (resize, focus, fonts, DPI) are debounced by this much before the grids are compared. */
export const DRIFT_DEBOUNCE_MS = 250;

export type DriftVerdict = 'none' | 'confirm' | 'correct';

/**
 * Decides, per check, whether to do nothing, look again shortly (`confirm`), or correct now.
 * Pure state: times are passed in.
 */
export class DriftMonitor {
  private pending: { key: string; since: number } | null = null;
  private lastCorrection = -Infinity;
  private attempts = new Map<string, number>();

  check(viewer: GridSize, pty: GridSize | null | undefined, following: boolean, now: number): DriftVerdict {
    if (!hasGridDrift(viewer, pty, following)) {
      this.pending = null;
      this.attempts.clear();
      return 'none';
    }
    const key = `${viewer.cols}x${viewer.rows}/${pty!.cols}x${pty!.rows}`;
    if (this.pending?.key !== key) {
      this.pending = { key, since: now };
      return 'confirm';
    }
    if (now - this.pending.since < DRIFT_CONFIRM_MS) return 'confirm';
    if (now - this.lastCorrection < DRIFT_COOLDOWN_MS) return 'none';
    const tried = this.attempts.get(key) ?? 0;
    if (tried >= DRIFT_MAX_ATTEMPTS) return 'none';
    this.attempts.set(key, tried + 1);
    this.lastCorrection = now;
    return 'correct';
  }
}
