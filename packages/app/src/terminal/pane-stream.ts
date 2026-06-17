import type { PaneDataListener } from './types';

export const MAX_BACKLOG_CHUNKS = 128;
export const RAW_REPLAY_LINES = 10_000;

export type PaneStreamMode = 'buffering' | 'live';

export interface PaneStream {
  mode: PaneStreamMode;
  backlog: Uint8Array[];
  truncated: boolean;
  listener: PaneDataListener | null;
}

export type ReadRawBufferFn = (paneId: string, lines: number) => Promise<number[]>;

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

  subscribe(paneId: string, cb: PaneDataListener, readRawBuffer: ReadRawBufferFn): () => void {
    const stream = this.streamFor(paneId);
    stream.listener = cb;

    if (stream.mode === 'live') {
      return this.makeUnsub(paneId, cb);
    }

    stream.truncated = false;
    stream.backlog = [];

    void readRawBuffer(paneId, RAW_REPLAY_LINES).then((raw) => {
      const current = this.streams.get(paneId);
      if (!current || current.listener !== cb || current.mode === 'live') return;
      if (raw.length > 0) {
        cb(Uint8Array.from(raw));
      }
      this.drainBacklogAndGoLive(current, cb);
    });

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
    };
  }

  private drainBacklogAndGoLive(stream: PaneStream, cb: PaneDataListener): void {
    const backlog = stream.backlog;
    stream.backlog = [];
    stream.mode = 'live';
    for (const chunk of backlog) {
      cb(chunk);
    }
  }

  private makeUnsub(paneId: string, cb: PaneDataListener): () => void {
    return () => {
      const current = this.streams.get(paneId);
      if (current && current.listener === cb) {
        current.listener = null;
        current.mode = 'buffering';
      }
    };
  }
}
