export interface GridSize {
  cols: number;
  rows: number;
}

export interface OwnerResizeDecision {
  /** The PTY size the owner window now follows, or null when it is at the size it asked for. */
  follow: GridSize | null;
  /** The size the owner's xterm grid must be resized to, or null when it already matches. */
  resizeTo: GridSize | null;
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
): OwnerResizeDecision {
  const follow =
    event.cols === requested.cols && event.rows === requested.rows ? null : { ...event };
  const differs = current.cols !== event.cols || current.rows !== event.rows;
  return {
    follow,
    resizeTo: differs ? { cols: event.cols, rows: event.rows } : null,
  };
}
