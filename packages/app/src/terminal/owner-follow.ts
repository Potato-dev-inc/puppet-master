export interface GridSize {
  cols: number;
  rows: number;
}

export interface OwnerResizeDecision {
  /** The PTY size the owner window now follows, or null when it is at the size it asked for. */
  follow: GridSize | null;
  /** The size the owner's xterm grid must be resized to, or null when it already matches. */
  resizeTo: GridSize | null;
  /**
   * The PTY just came back to the size this window asked for (a reclaim or a release): the window
   * must measure its container again against the PTY's real grid, because the size the PTY was
   * given back may not be the one that fits the window now.
   */
  refit: boolean;
}

/**
 * What an owner (desktop pop-out) window does with a PTY resize that arrived in stream order.
 *
 * While a phone holds the pane the PTY has a size other than the one the window asked for, so the
 * window's xterm grid must follow the PTY (ConPTY repaints by absolute cursor position; a grid that
 * differs from the PTY at any point of the stream leaves garbage behind). Once the PTY is back at
 * the requested size, following ends.
 */
export function ownerResizeDecision(
  event: GridSize,
  requested: GridSize,
  current: GridSize,
  wasFollowing = false,
): OwnerResizeDecision {
  const follow =
    event.cols === requested.cols && event.rows === requested.rows ? null : { ...event };
  const differs = current.cols !== event.cols || current.rows !== event.rows;
  return {
    follow,
    resizeTo: differs ? { cols: event.cols, rows: event.rows } : null,
    refit: wasFollowing && follow === null,
  };
}

/**
 * The owner window's bookkeeping of the PTY grid: what it last asked for, what the PTY has, and
 * whether it follows a size another party (a phone) gave the PTY. Pure state, so the transitions
 * (lease, release, reclaim) are testable without a DOM.
 */
export class OwnerGrid {
  /** The size this window last sent to the app (0x0 until it sent one). */
  lastSent: GridSize = { cols: 0, rows: 0 };
  /** The PTY size this window follows while a phone holds it; null when the PTY is at `lastSent`. */
  follow: GridSize | null = null;
  /** The PTY's grid as the last in-order resize reported it. */
  pty: GridSize | null = null;

  get following(): boolean {
    return this.follow !== null;
  }

  /** Forget what was sent: the next fit is sent again even at the same size (the PTY may not have it). */
  forgetSent(): void {
    this.lastSent = { cols: 0, rows: 0 };
  }

  /** A fit measured the container: the size to send to the app, or null when it already has it. */
  fitted(cols: number, rows: number): GridSize | null {
    if (cols <= 0 || rows <= 0) return null;
    if (cols === this.lastSent.cols && rows === this.lastSent.rows) return null;
    this.lastSent = { cols, rows };
    return { cols, rows };
  }

  /** A PTY resize in stream order. `current` is this window's xterm grid. */
  onStreamResize(event: GridSize, current: GridSize): OwnerResizeDecision {
    this.pty = { ...event };
    const decision = ownerResizeDecision(event, this.lastSent, current, this.follow !== null);
    this.follow = decision.follow;
    // The PTY is at the size this window asked for again: that is what it holds now.
    if (decision.refit) this.lastSent = { ...event };
    return decision;
  }

  /**
   * This window took the pane back from a phone (`reclaim_pane_size` resolved). The size the app
   * gave the PTY back may be one this window never sent (remembered from another window), and what
   * this window last sent says nothing about it: trust the PTY's reported grid, so the next fit is
   * compared against what the PTY really has and is sent when it differs (once).
   */
  reclaimed(): void {
    this.follow = null;
    this.lastSent = this.pty ? { ...this.pty } : { cols: 0, rows: 0 };
  }
}
