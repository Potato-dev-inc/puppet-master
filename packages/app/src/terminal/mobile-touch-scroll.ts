import type { Disposable } from './types';

const TAP_THRESHOLD_PX = 10;

export interface MobileTouchScrollOptions {
  container: HTMLElement;
  lineHeightPx: number;
  scrollLines: (lines: number) => void;
  shouldIgnoreTarget?: (target: EventTarget | null) => boolean;
  /** Called on pointer up when the gesture was a tap, not a scroll drag. */
  onBackgroundTap?: (target: EventTarget | null) => void;
}

/**
 * Vertical drag-to-scroll for mobile terminal viewers. Swiping up/down scrolls
 * xterm history; a short tap without movement triggers onBackgroundTap.
 */
export class MobileTouchScroll implements Disposable {
  private readonly container: HTMLElement;
  private readonly lineHeightPx: number;
  private readonly scrollLines: (lines: number) => void;
  private readonly shouldIgnoreTarget?: (target: EventTarget | null) => boolean;
  private readonly onBackgroundTap?: (target: EventTarget | null) => void;
  private activePointerId: number | null = null;
  private startY = 0;
  private lastY = 0;
  private scrollRemainderPx = 0;
  private dragging = false;
  private tapTarget: EventTarget | null = null;

  constructor(options: MobileTouchScrollOptions) {
    this.container = options.container;
    this.lineHeightPx = Math.max(12, options.lineHeightPx);
    this.scrollLines = options.scrollLines;
    this.shouldIgnoreTarget = options.shouldIgnoreTarget;
    this.onBackgroundTap = options.onBackgroundTap;

    this.container.addEventListener('pointerdown', this.onPointerDown);
    this.container.addEventListener('pointermove', this.onPointerMove);
    this.container.addEventListener('pointerup', this.onPointerUp);
    this.container.addEventListener('pointercancel', this.onPointerUp);
  }

  dispose(): void {
    this.container.removeEventListener('pointerdown', this.onPointerDown);
    this.container.removeEventListener('pointermove', this.onPointerMove);
    this.container.removeEventListener('pointerup', this.onPointerUp);
    this.container.removeEventListener('pointercancel', this.onPointerUp);
  }

  private readonly onPointerDown = (event: PointerEvent): void => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    if (this.shouldIgnoreTarget?.(event.target)) return;

    this.activePointerId = event.pointerId;
    this.startY = event.clientY;
    this.lastY = event.clientY;
    this.scrollRemainderPx = 0;
    this.dragging = false;
    this.tapTarget = event.target;
  };

  private readonly onPointerMove = (event: PointerEvent): void => {
    if (event.pointerId !== this.activePointerId) return;
    if (this.shouldIgnoreTarget?.(event.target)) return;

    const dy = event.clientY - this.lastY;
    this.lastY = event.clientY;

    if (!this.dragging && Math.abs(event.clientY - this.startY) >= TAP_THRESHOLD_PX) {
      this.dragging = true;
      this.trySetPointerCapture(event.pointerId);
    }

    if (!this.dragging) return;

    event.preventDefault();
    this.scrollRemainderPx += dy;
    const lines = Math.trunc(this.scrollRemainderPx / this.lineHeightPx);
    if (lines !== 0) {
      this.scrollLines(lines);
      this.scrollRemainderPx -= lines * this.lineHeightPx;
    }
  };

  private readonly onPointerUp = (event: PointerEvent): void => {
    if (event.pointerId !== this.activePointerId) return;

    if (this.dragging) {
      event.preventDefault();
    } else {
      this.onBackgroundTap?.(this.tapTarget);
    }

    this.tryReleasePointerCapture(event.pointerId);

    this.activePointerId = null;
    this.dragging = false;
    this.tapTarget = null;
    this.scrollRemainderPx = 0;
  };

  private trySetPointerCapture(pointerId: number): void {
    try {
      this.container.setPointerCapture?.(pointerId);
    } catch {
      /* ignore */
    }
  }

  private tryReleasePointerCapture(pointerId: number): void {
    try {
      if (this.container.hasPointerCapture?.(pointerId)) {
        this.container.releasePointerCapture(pointerId);
      }
    } catch {
      /* ignore */
    }
  }
}
