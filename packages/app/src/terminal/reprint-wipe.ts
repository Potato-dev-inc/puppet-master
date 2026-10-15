interface Grid {
  cols: number;
  rows: number;
}

/** The part of an xterm.js terminal the wipe needs. */
export interface WipeTarget {
  cols: number;
  rows: number;
  write(data: string): void;
}

/** ED 3: erase the saved lines (scrollback). The visible screen is left to the PTY's repaint. */
export const CLEAR_SCROLLBACK = '\x1b[3J';

/**
 * Agents whose TUI reprints its whole conversation on every resize without clearing scrollback
 * (`TuiProfile::reprints_on_resize`, Claude Code) leave one more copy of the conversation in a
 * viewer's history at every PTY resize. The viewer wipes its saved lines at the resize point of the
 * byte stream, so the reprint that follows is the only copy. Agents that do not reprint keep their
 * history.
 *
 * The wipe is a write: it queues in order with the pane's bytes, so it lands exactly between the
 * output drawn for the old grid and the output drawn for the new one. Only a real change of the
 * grid wipes; the grid compared against is the one the viewer will have once everything queued so
 * far is parsed.
 */
export class ReprintWipe {
  enabled = false;
  /** The grid after everything queued so far; null until the first resize is seen. */
  private grid: Grid | null = null;

  /** The viewer is rebuilt or reset: forget the tracked grid. */
  reset(): void {
    this.grid = null;
  }

  /**
   * The grid becomes `cols`x`rows` at this point of the stream. `before` is the grid just before,
   * when the terminal already moved (a local fit); otherwise the tracked grid or the terminal's own.
   * Returns true when scrollback was wiped.
   */
  resizeTo(term: WipeTarget, cols: number, rows: number, before?: Grid): boolean {
    const prev = this.grid ?? before ?? { cols: term.cols, rows: term.rows };
    this.grid = { cols, rows };
    if (!this.enabled || (prev.cols === cols && prev.rows === rows)) return false;
    term.write(CLEAR_SCROLLBACK);
    return true;
  }
}
