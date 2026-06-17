import type { Disposable } from './types';

export interface SnapshotRenderer {
  setSnapshot(snapshot: string): void;
}

/**
 * Coalesces rapid snapshot updates into one render per animation frame.
 */
export class SnapshotBatcher implements Disposable {
  private pending: string | null = null;
  private frame: number | null = null;

  constructor(private readonly term: SnapshotRenderer) {}

  push(snapshot: string): void {
    this.pending = snapshot;
    if (this.frame === null) {
      this.frame = requestAnimationFrame(() => this.flush());
    }
  }

  dispose(): void {
    if (this.frame !== null) {
      cancelAnimationFrame(this.frame);
      this.frame = null;
    }
    this.pending = null;
  }

  private flush(): void {
    this.frame = null;
    if (this.pending === null) return;
    const snapshot = this.pending;
    this.pending = null;
    this.term.setSnapshot(snapshot);
  }
}
