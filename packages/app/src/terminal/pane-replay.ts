import type { PaneScreenReplay, ReadRawBufferFn } from './pane-stream';

export interface OwnerReplayDeps {
  /** The pane redrawn at its current size, or null when the backend has no such command. */
  readScreen: (paneId: string) => Promise<{ data: number[] | Uint8Array; cols: number; rows: number; offset?: number } | null>;
  readRaw: ReadRawBufferFn;
  /** This window's own resize of the pane that is still in flight (the PTY has not applied it yet). */
  pendingResize: (paneId: string) => Promise<unknown> | undefined;
}

/**
 * What a desktop window replays when it mounts a pane (cockpit or pop-out). The raw byte history
 * was written while the PTY had other sizes (phone lease, pop-out, cockpit): replayed into one
 * grid, ConPTY's absolute cursor positions lay narrow-wrapped text over wide text and every mode
 * switch added a garbled copy. The screen replay is the pane redrawn at its current size, together
 * with that size, so the viewer's grid equals the PTY grid from its first byte on.
 *
 * Waits for this window's own mount-time resize first, so the grid the screen is drawn for is the
 * one the window asked for (the owner's own resizes are not echoed back as `pty://resized`).
 */
export function createOwnerReplayReader(deps: OwnerReplayDeps): ReadRawBufferFn {
  return async (paneId, lines) => {
    try {
      await deps.pendingResize(paneId);
    } catch {
      /* the resize failed: the screen still reports the real grid */
    }
    try {
      const screen = await deps.readScreen(paneId);
      if (screen) {
        const replay: PaneScreenReplay = {
          data: screen.data instanceof Uint8Array ? screen.data : Uint8Array.from(screen.data),
          cols: screen.cols,
          rows: screen.rows,
          offset: screen.offset,
        };
        return replay;
      }
    } catch {
      /* an older backend without the command: raw history below */
    }
    return deps.readRaw(paneId, lines);
  };
}
