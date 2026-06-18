import type { Disposable } from './types';

/** Debounces keystrokes before forwarding them to the PTY. */
export class InputBatcher implements Disposable {
  private pending = '';
  private timer: number | null = null;

  constructor(
    private readonly flush: (text: string) => void,
    private readonly delayMs = 8,
  ) {}

  push(data: string): void {
    this.pending += data;
    if (this.timer === null) {
      this.timer = window.setTimeout(() => this.drain(), this.delayMs);
    }
  }

  dispose(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.drain();
  }

  /** Flush any debounced keystrokes immediately (e.g. before mobile word replacement). */
  flushNow(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.drain();
  }

  private drain(): void {
    this.timer = null;
    const text = this.pending;
    this.pending = '';
    if (text.length > 0) {
      this.flush(text);
    }
  }
}
