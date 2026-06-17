import type { Disposable } from './types';

export interface TerminalWriter {
  write(data: Uint8Array): void;
}

/**
 * Coalesces PTY byte chunks into merged writes on animation frames.
 *
 * The terminal's `write` is synchronous in our custom renderer, but we still
 * batch so that many small PTY chunks are parsed and rendered together,
 * preventing partial-frame flicker.
 */
export class WriteBatcher implements Disposable {
  private pending: Uint8Array[] = [];
  private writeFrame: number | null = null;
  private writing = false;

  constructor(private readonly term: TerminalWriter) {}

  push(data: Uint8Array): void {
    this.pending.push(data);
    this.schedule();
  }

  dispose(): void {
    if (this.writeFrame !== null) {
      cancelAnimationFrame(this.writeFrame);
      this.writeFrame = null;
    }
    this.pending = [];
    this.writing = false;
  }

  private schedule(): void {
    if (this.writeFrame !== null || this.writing) return;
    this.writeFrame = requestAnimationFrame(() => this.flush());
  }

  private flush(): void {
    this.writeFrame = null;
    if (this.writing || this.pending.length === 0) return;

    const chunks = this.pending;
    this.pending = [];
    const merged = mergeChunks(chunks);

    this.writing = true;
    try {
      this.term.write(merged);
    } finally {
      this.writing = false;
      if (this.pending.length > 0) {
        this.schedule();
      }
    }
  }
}

export function mergeChunks(chunks: Uint8Array[]): Uint8Array {
  const totalLength = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const merged = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return merged;
}
