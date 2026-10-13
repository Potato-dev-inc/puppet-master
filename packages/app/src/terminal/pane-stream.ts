import type { PaneDataListener, PaneResizeListener } from './types';

export const MAX_BACKLOG_CHUNKS = 128;
/** While a replay is loading for a subscriber: generous, a replay takes tens of milliseconds. */
const MAX_LOADING_CHUNKS = 8192;
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
  pendingResize: { cols: number; rows: number; offset?: number } | null;
  /** Stream offset of each `backlog` chunk (bytes since the pane started); undefined when unknown. */
  backlogOffsets: Array<number | undefined>;
  /** Resizes that arrived while the replay was loading, with how many chunks preceded each. */
  resizeLog: Array<{ cols: number; rows: number; offset?: number; after: number }>;
}

/** The pane redrawn at its current size (`GET /panes/{id}/screen`): replayed after a resize to it. */
export interface PaneScreenReplay {
  data: Uint8Array;
  cols: number;
  rows: number;
  /**
   * Stream offset the snapshot was taken at: every byte before it is drawn in `data`. Chunks
   * (`ingest`'s `offset`) and resizes carry offsets of the same stream, so the hand-over to live
   * output loses nothing and writes nothing twice. Absent: the backlog cannot be placed and is dropped.
   */
  offset?: number;
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

  /** `offset`: where this chunk starts in the pane's byte stream (bytes since the pane started). */
  ingest(paneId: string, data: Uint8Array | number[], offset?: number): void {
    const chunk = copyBytes(data);
    const stream = this.streamFor(paneId);

    if (stream.mode === 'live') {
      stream.listener?.(chunk);
      return;
    }

    stream.backlog.push(chunk);
    stream.backlogOffsets.push(offset);
    // A replay in flight must not lose the chunks it will be joined with: only a pane nobody is
    // looking at keeps just the latest ones.
    const cap = stream.listener ? MAX_LOADING_CHUNKS : MAX_BACKLOG_CHUNKS;
    while (stream.backlog.length > cap) {
      stream.backlog.shift();
      stream.backlogOffsets.shift();
      for (const entry of stream.resizeLog) entry.after = Math.max(0, entry.after - 1);
      stream.truncated = true;
    }
  }

  /**
   * A pane resize, delivered between the chunks it came between. While the replay is loading it is
   * remembered: opening a tab claims the pane for the phone, so the PTY often resizes while the
   * screen is in flight, and a snapshot taken before that resize must not set the viewer's size.
   */
  ingestResize(paneId: string, cols: number, rows: number, offset?: number): void {
    const stream = this.streams.get(paneId);
    if (!stream) return;
    if (stream.mode === 'live') {
      stream.resizeListener?.(cols, rows);
      return;
    }
    stream.pendingResize = { cols, rows, offset };
    if (stream.listener) stream.resizeLog.push({ cols, rows, offset, after: stream.backlog.length });
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
    stream.backlogOffsets = [];
    stream.resizeLog = [];
    stream.pendingResize = null;

    const load = (attempt: number): void => {
      void readRawBuffer(paneId, RAW_REPLAY_LINES).then((replay) => {
        const current = this.streams.get(paneId);
        if (!current || current.listener !== cb || current.mode === 'live') return;
        let pending = current.pendingResize;
        current.pendingResize = null;
        if (isScreenReplay(replay)) {
          const snapshotAt = replay.offset;
          // A resize placed after the snapshot is replayed in order below; only one the snapshot
          // may not have seen (before it, or unplaced) makes the screen stale.
          if (snapshotAt !== undefined && pending?.offset !== undefined && pending.offset >= snapshotAt) {
            const stale = [...current.resizeLog].reverse().find((r) => r.offset !== undefined && r.offset < snapshotAt);
            pending = stale ? { cols: stale.cols, rows: stale.rows, offset: stale.offset } : null;
          }
          // The PTY resized after this snapshot was taken: it shows the old grid, and the repaint
          // for the new one is in the backlog that going live drops. Take the screen again.
          if (pending && (pending.cols !== replay.cols || pending.rows !== replay.rows) && attempt < MAX_REPLAY_ATTEMPTS) {
            load(attempt + 1);
            return;
          }
          // Size the viewer to the grid the screen was drawn for, then draw it.
          current.resizeListener?.(replay.cols, replay.rows);
          if (replay.data.length > 0) cb(replay.data);
          if (snapshotAt !== undefined) {
            this.joinLive(current, cb, snapshotAt);
            return;
          }
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
      backlogOffsets: [],
      resizeLog: [],
    };
  }

  /**
   * Hand the viewer over to live output after a snapshot taken at stream offset `snapshotAt`:
   * chunks (and resizes) that arrived while it loaded are written in order, minus the bytes the
   * snapshot already drew. Nothing is lost and nothing is drawn twice.
   */
  private joinLive(stream: PaneStream, cb: PaneDataListener, snapshotAt: number): void {
    const { backlog, backlogOffsets, resizeLog } = stream;
    let next = 0;
    const applyResizesBefore = (index: number): void => {
      while (next < resizeLog.length && resizeLog[next].after <= index) {
        const r = resizeLog[next++];
        if (r.offset === undefined || r.offset >= snapshotAt) stream.resizeListener?.(r.cols, r.rows);
      }
    };
    for (let i = 0; i < backlog.length; i++) {
      applyResizesBefore(i);
      const chunk = backlog[i];
      const start = backlogOffsets[i];
      if (start === undefined) {
        cb(chunk);
      } else if (start + chunk.length > snapshotAt) {
        cb(start < snapshotAt ? chunk.subarray(snapshotAt - start) : chunk);
      }
    }
    applyResizesBefore(Number.MAX_SAFE_INTEGER);
    stream.backlog = [];
    stream.backlogOffsets = [];
    stream.resizeLog = [];
    stream.mode = 'live';
  }

  private discardBacklogAndGoLive(stream: PaneStream): void {
    stream.backlog = [];
    stream.backlogOffsets = [];
    stream.resizeLog = [];
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
