import type { CanvasTerminal } from './canvas-terminal';
import type { Disposable, TerminalResizeHandler } from './types';

/**
 * Keeps the terminal grid aligned with its host element and notifies the PTY
 * backend when cols/rows change.
 */
export class ResizeController implements Disposable {
  private readonly observer: ResizeObserver;
  private resizeFrame: number | null = null;
  private resizeUnsub: (() => void) | null = null;

  constructor(
    private readonly container: HTMLElement,
    private readonly term: CanvasTerminal,
    private readonly onResize: TerminalResizeHandler,
  ) {
    this.resizeUnsub = this.term.onResize((cols: number, rows: number) => {
      this.onResize(cols, rows);
    });

    this.observer = new ResizeObserver(() => {
      if (this.resizeFrame !== null) return;
      this.resizeFrame = requestAnimationFrame(() => {
        this.resizeFrame = null;
        this.term.fit();
      });
    });
    this.observer.observe(container);

    this.term.fit();
  }

  fitNow(): void {
    this.term.fit();
  }

  dispose(): void {
    this.observer.disconnect();
    if (this.resizeFrame !== null) {
      cancelAnimationFrame(this.resizeFrame);
      this.resizeFrame = null;
    }
    this.resizeUnsub?.();
    this.resizeUnsub = null;
  }
}
