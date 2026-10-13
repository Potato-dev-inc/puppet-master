import type { PaneDataListener, PaneResizeListener } from './types';

export const MAX_BACKLOG_CHUNKS = 128;
export const RAW_REPLAY_LINES = 10_000;
/** Screen fetches per subscribe when the PTY keeps resizing while one is in flight. */
const MAX_REPLAY_ATTEMPTS = 3;

export type PaneStreamMode = 'buffering' | 'live';

export interface PaneStream {
  mode: PaneStreamMode;
  backlog: Uint8Array[];
  truncated: boolean;
  listener: PaneDataListener | null;
  resizeListener: PaneResizeListener | null;
  /** The latest resize that arrived while the replay was loading. */
  pendingResize: { cols: number; rows: number } | null;
}

/** The pane redrawn at its current size (`GET /panes/{id}/screen`): replayed after a resize to it. */
export interface PaneScreenReplay {
  data: Uint8Array;
  cols: number;
  rows: number;
}

export type ReadRawBufferFn = (
  paneId: string,
  lines: number,
) => Promise<number[] | Uint8Array | PaneScreenReplay>;

function isScreenReplay(value: number[] | Uint8Array | PaneScreenReplay): value is PaneScreenReplay {
  return !Array.isArray(value) && !(value instanceof Uint8Array);
}

function copyBytes(data: Uint8Array | number[]): Uint8Array {
  return data instanceof Uint8Array ? new Uint8Array(data) : Uint8Array.from(data);
}

/**
 * Per-pane raw-byte delivery for xterm.js. Detached panes buffer recent chunks;
 * if the backlog overflows, a subscriber replays raw backend scrollback before
 * switching back to live bytes.
 */
export class PaneStreamManager {
  private readonly streams = new Map<string, PaneStream>();

  ingest(paneId: string, data: Uint8Array | number[]): void {
    const chunk = copyBytes(data);
    const stream = this.streamFor(paneId);

    if (stream.mode === 'live') {
      stream.listener?.(chunk);
      return;
    }

    stream.backlog.push(chunk);
    while (stream.backlog.length > MAX_BACKLOG_CHUNKS) {
      stream.backlog.shift();
      stream.truncated = true;
    }
  }

  /**
   * A pane resize, delivered between the chunks it came between. While the replay is loading it is
   * remembered: opening a tab claims the pane for the phone, so the PTY often resizes while the
   * screen is in flight, and a snapshot taken before that resize must not set the viewer's size.
   */
  ingestResize(paneId: string, cols: number, rows: number): void {
    const stream = this.streams.get(paneId);
    if (!stream) return;
    if (stream.mode === 'live') stream.resizeListener?.(cols, rows);
    else stream.pendingResize = { cols, rows };
  }

  subscribe(
    paneId: string,
    cb: PaneDataListener,
    readRawBuffer: ReadRawBufferFn,
    onResize?: PaneResizeListener,
  ): () => void {
    const stream = this.streamFor(paneId);
    stream.listener = cb;
    stream.resizeListener = onResize ?? null;

    if (stream.mode === 'live') {
      return this.makeUnsub(paneId, cb);
    }

    stream.truncated = false;
    stream.backlog = [];
    stream.pendingResize = null;

    const load = (attempt: number): void => {
      void readRawBuffer(paneId, RAW_REPLAY_LINES).then((replay) => {
        const current = this.streams.get(paneId);
        if (!current || current.listener !== cb || current.mode === 'live') return;
        const pending = current.pendingResize;
        current.pendingResize = null;
        if (isScreenReplay(replay)) {
          // The PTY resized after this snapshot was taken: it shows the old grid, and the repaint
          // for the new one is in the backlog that going live drops. Take the screen again.
          if (pending && (pending.cols !== replay.cols || pending.rows !== replay.rows) && attempt < MAX_REPLAY_ATTEMPTS) {
            load(attempt + 1);
            return;
          }
          // Size the viewer to the grid the screen was drawn for, then draw it.
          current.resizeListener?.(replay.cols, replay.rows);
          if (replay.data.length > 0) cb(replay.data);
        } else {
          if (replay.length > 0) cb(Uint8Array.from(replay));
          if (pending) current.resizeListener?.(pending.cols, pending.rows);
        }
        this.discardBacklogAndGoLive(current);
      });
    };
    load(1);

    return this.makeUnsub(paneId, cb);
  }

  reset(paneId: string): void {
    this.streams.set(paneId, this.emptyStream());
  }

  delete(paneId: string): void {
    this.streams.delete(paneId);
  }

  private streamFor(paneId: string): PaneStream {
    let stream = this.streams.get(paneId);
    if (!stream) {
      stream = this.emptyStream();
      this.streams.set(paneId, stream);
    }
    return stream;
  }

  private emptyStream(): PaneStream {
    return {
      mode: 'buffering',
      backlog: [],
      truncated: false,
      listener: null,
      resizeListener: null,
      pendingResize: null,
    };
  }

  private discardBacklogAndGoLive(stream: PaneStream): void {
    stream.backlog = [];
    stream.mode = 'live';
  }

  private makeUnsub(paneId: string, cb: PaneDataListener): () => void {
    return () => {
      const current = this.streams.get(paneId);
      if (current && current.listener === cb) {
        current.listener = null;
        current.resizeListener = null;
        current.mode = 'buffering';
      }
    };
  }
}
