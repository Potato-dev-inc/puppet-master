// Swipe-to-scroll for the phone PWA terminal. A vertical one-finger drag scrolls: full-screen
// agents that capture the mouse get wheel events, normal-buffer output scrolls xterm's own
// scrollback, and the alternate buffer without mouse tracking is left alone.
import type { Terminal } from '@xterm/xterm';

/** Travel (px) before a touch counts as a scroll rather than a tap. */
export const TOUCH_SCROLL_START_PX = 8;
/** Most wheel steps sent for one touchmove or momentum frame. */
export const MAX_WHEEL_STEPS_PER_FRAME = 10;
/** Velocity retained per 16 ms frame after touchend. */
export const MOMENTUM_FRICTION = 0.95;
/** Momentum stops once it drops below this speed (px per ms). */
export const MOMENTUM_MIN_VELOCITY = 0.02;
const MOMENTUM_MAX_VELOCITY = 3;
const FLING_SAMPLE_MS = 80;
/** Largest column/row the legacy X10 mouse encoding can carry. */
const X10_MAX_COORD = 223;

/** Class of the floating "scroll to bottom" button. */
export const TOUCH_SCROLL_BOTTOM_CLASS = 'terminal-scroll-bottom';
export const TOUCH_SCROLL_BOTTOM_VISIBLE_CLASS = 'terminal-scroll-bottom--visible';

/** Elements whose touches must not start a scroll (they have their own handling). */
const IGNORED_TOUCH_TARGETS =
  '[data-mobile-terminal-arrows], [data-mobile-terminal-input-zone], [data-terminal-scroll-bottom]';

/**
 * Mouse-wheel report for one step. `col` and `row` are 1-based cells. SGR (1006) is the modern
 * encoding; X10 is the legacy fallback.
 */
export function wheelSequence(direction: 'up' | 'down', col: number, row: number, sgr: boolean): string {
  const button = direction === 'up' ? 64 : 65;
  if (sgr) {
    return `\x1b[<${button};${col};${row}M`;
  }
  const x = Math.min(Math.max(col, 1), X10_MAX_COORD);
  const y = Math.min(Math.max(row, 1), X10_MAX_COORD);
  return `\x1b[M${String.fromCharCode(32 + button)}${String.fromCharCode(32 + x)}${String.fromCharCode(32 + y)}`;
}

/**
 * Whole rows covered by the accumulated drag, and the leftover pixels carried to the next move.
 * Positive input means the content moved down (finger down, older output).
 */
export function linesForDelta(accumulatedPx: number, cellHeightPx: number): { lines: number; remainderPx: number } {
  if (!(cellHeightPx > 0)) return { lines: 0, remainderPx: accumulatedPx };
  const lines = Math.trunc(accumulatedPx / cellHeightPx);
  return { lines, remainderPx: accumulatedPx - lines * cellHeightPx };
}

export interface TouchScrollOptions {
  /** Sends raw bytes to the PTY (mouse-wheel reports). */
  sendInput: (data: string) => void;
  /** Height of one terminal row, in the same pixels as touch coordinates. */
  cellHeight: () => number;
  /** 1-based cell under a viewport point. */
  cellFromPoint: (clientX: number, clientY: number) => { col: number; row: number };
  /** Called once when a drag becomes a scroll (lets the input guard drop its long-press). */
  onScrollStart?: () => void;
  /**
   * Pans the cropped (bottom-anchored) screen by a drag, in px (positive = reveal the top). Returns
   * the part not consumed, which is then scrolled as usual. Called first for every drag, except
   * when the drag scrolls xterm back down toward its bottom.
   */
  panViewport?: (dyPx: number) => number;
  /** True while the cropped screen is panned (keeps the jump-to-bottom button visible). */
  isPanned?: () => boolean;
  /** Returns the cropped screen to its bottom-anchored position. */
  resetPan?: () => void;
}

interface Gesture {
  id: number;
  startX: number;
  startY: number;
  lastY: number;
  active: boolean;
}

export class TouchScrollController {
  private readonly container: HTMLElement;
  private readonly terminal: Terminal;
  private readonly options: TouchScrollOptions;
  private readonly bottomButton: HTMLButtonElement;
  private readonly disposers: Array<() => void> = [];
  private readonly previousTouchAction: string;
  private gesture: Gesture | null = null;
  private lastX = 0;
  private lastY = 0;
  private remainderPx = 0;
  private velocity = 0;
  private lastMoveTime = 0;
  private momentumFrame: number | null = null;
  private disposed = false;

  constructor(container: HTMLElement, terminal: Terminal, options: TouchScrollOptions) {
    this.container = container;
    this.terminal = terminal;
    this.options = options;

    // Vertical pans are ours; the browser keeps horizontal panning and does not zoom the terminal.
    this.previousTouchAction = container.style.touchAction;
    container.style.touchAction = 'pan-x';

    this.bottomButton = document.createElement('button');
    this.bottomButton.type = 'button';
    this.bottomButton.className = TOUCH_SCROLL_BOTTOM_CLASS;
    this.bottomButton.setAttribute('data-terminal-scroll-bottom', 'true');
    this.bottomButton.setAttribute('aria-label', 'Scroll to bottom');
    this.bottomButton.textContent = '↓';
    container.appendChild(this.bottomButton);
    const onBottomClick = (event: MouseEvent): void => {
      event.preventDefault();
      event.stopPropagation();
      this.options.resetPan?.();
      this.terminal.scrollToBottom();
      this.updateBottomButton();
    };
    this.bottomButton.addEventListener('click', onBottomClick);
    this.disposers.push(() => this.bottomButton.removeEventListener('click', onBottomClick));

    const onTouchStart = (event: TouchEvent): void => this.handleTouchStart(event);
    const onTouchMove = (event: TouchEvent): void => this.handleTouchMove(event);
    const onTouchEnd = (event: TouchEvent): void => this.handleTouchEnd(event);
    // Capture phase: the input guard stops propagation of the press it handles, and xterm's own
    // touch scrolling (below us in the tree) must not run as well.
    container.addEventListener('touchstart', onTouchStart, { passive: true, capture: true });
    container.addEventListener('touchmove', onTouchMove, { passive: false, capture: true });
    container.addEventListener('touchend', onTouchEnd, { passive: true, capture: true });
    container.addEventListener('touchcancel', onTouchEnd, { passive: true, capture: true });
    this.disposers.push(() => {
      container.removeEventListener('touchstart', onTouchStart, { capture: true });
      container.removeEventListener('touchmove', onTouchMove, { capture: true });
      container.removeEventListener('touchend', onTouchEnd, { capture: true });
      container.removeEventListener('touchcancel', onTouchEnd, { capture: true });
    });

    const scrollSub = this.terminal.onScroll(() => this.updateBottomButton());
    const writeSub = this.terminal.onWriteParsed(() => this.updateBottomButton());
    this.disposers.push(() => {
      scrollSub.dispose();
      writeSub.dispose();
    });
    this.updateBottomButton();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.stopMomentum();
    this.gesture = null;
    for (const dispose of this.disposers.splice(0)) dispose();
    this.bottomButton.remove();
    this.container.style.touchAction = this.previousTouchAction;
  }

  private handleTouchStart(event: TouchEvent): void {
    this.stopMomentum();
    this.velocity = 0;
    this.remainderPx = 0;
    this.gesture = null;
    if (event.touches.length !== 1) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest(IGNORED_TOUCH_TARGETS)) return;
    const touch = event.touches[0];
    this.gesture = {
      id: touch.identifier,
      startX: touch.clientX,
      startY: touch.clientY,
      lastY: touch.clientY,
      active: false,
    };
    this.lastX = touch.clientX;
    this.lastY = touch.clientY;
  }

  private handleTouchMove(event: TouchEvent): void {
    const gesture = this.gesture;
    if (!gesture) return;
    if (event.touches.length !== 1) {
      // Two fingers (pinch and the like) belong to the browser.
      this.gesture = null;
      return;
    }
    // One-finger drags are ours: keep them from xterm's own touch scrolling.
    event.stopPropagation();
    const touch = Array.from(event.touches).find((t) => t.identifier === gesture.id) ?? event.touches[0];
    const dx = touch.clientX - gesture.startX;
    const dy = touch.clientY - gesture.startY;

    if (!gesture.active) {
      if (Math.abs(dy) <= TOUCH_SCROLL_START_PX || Math.abs(dy) <= Math.abs(dx)) {
        // Still a tap or a horizontal drag: leave it alone, and stop tracking a horizontal drag.
        if (Math.abs(dx) > TOUCH_SCROLL_START_PX) this.gesture = null;
        return;
      }
      gesture.active = true;
      gesture.lastY = gesture.startY;
      this.options.onScrollStart?.();
    }

    event.preventDefault();
    const now = performance.now();
    const step = touch.clientY - gesture.lastY;
    gesture.lastY = touch.clientY;
    this.lastX = touch.clientX;
    this.lastY = touch.clientY;
    const elapsed = now - this.lastMoveTime;
    if (elapsed > 0 && elapsed < FLING_SAMPLE_MS * 2) {
      this.velocity = step / elapsed;
    }
    this.lastMoveTime = now;
    this.applyDelta(step);
  }

  private handleTouchEnd(event: TouchEvent): void {
    const gesture = this.gesture;
    this.gesture = null;
    if (!gesture?.active) return;
    if (event.touches.length > 0 || performance.now() - this.lastMoveTime > FLING_SAMPLE_MS) {
      this.velocity = 0;
    }
    this.startMomentum();
  }

  /** `px` > 0 means the finger moved down: the content moves down, older output comes into view. */
  private applyDelta(px: number): void {
    const rest = this.takeViewportPan(px);
    const { lines, remainderPx } = linesForDelta(this.remainderPx + rest, this.options.cellHeight());
    this.remainderPx = remainderPx;
    if (lines === 0) {
      this.updateBottomButton();
      return;
    }
    const clamped = Math.max(-MAX_WHEEL_STEPS_PER_FRAME, Math.min(MAX_WHEEL_STEPS_PER_FRAME, lines));
    if (clamped !== lines) this.remainderPx = 0;
    this.scrollRows(clamped);
    this.updateBottomButton();
  }

  /**
   * The part of a drag left for scrolling after the cropped screen takes its share. Finger down
   * (older content): pan first, then scroll. Finger up: scroll xterm back toward its bottom while it
   * is scrolled up, and only then un-pan. A TUI that captures the mouse has no scrollback to
   * scroll, so its wheel events always come after the pan.
   */
  private takeViewportPan(px: number): number {
    const pan = this.options.panViewport;
    if (!pan) return px;
    const buffer = this.terminal.buffer.active;
    const mouse = this.terminal.modes.mouseTrackingMode !== 'none';
    if (px < 0 && !mouse && buffer.type === 'normal' && buffer.viewportY < buffer.baseY) {
      return px;
    }
    return pan(px);
  }

  private scrollRows(lines: number): void {
    if (this.terminal.modes.mouseTrackingMode !== 'none') {
      const direction = lines > 0 ? 'up' : 'down';
      const { col, row } = this.options.cellFromPoint(this.lastX, this.lastY);
      const sgr = this.usesSgrMouse();
      for (let i = 0; i < Math.abs(lines); i += 1) {
        this.options.sendInput(wheelSequence(direction, col, row, sgr));
      }
      return;
    }
    // Full-screen app without mouse support: nothing sensible to send.
    if (this.terminal.buffer.active.type === 'alternate') return;
    this.terminal.scrollLines(-lines);
  }

  private usesSgrMouse(): boolean {
    // xterm does not expose the mouse encoding publicly; default to SGR (all modern TUIs use 1006).
    const core = (this.terminal as unknown as {
      _core?: { coreMouseService?: { activeEncoding?: string } };
    })._core;
    const encoding = core?.coreMouseService?.activeEncoding;
    return encoding === undefined ? true : encoding !== 'DEFAULT';
  }

  private startMomentum(): void {
    this.stopMomentum();
    this.velocity = Math.max(-MOMENTUM_MAX_VELOCITY, Math.min(MOMENTUM_MAX_VELOCITY, this.velocity));
    if (Math.abs(this.velocity) < MOMENTUM_MIN_VELOCITY) return;
    let last = performance.now();
    const frame = (): void => {
      this.momentumFrame = null;
      if (this.disposed) return;
      const now = performance.now();
      const elapsed = Math.min(now - last, 64);
      last = now;
      this.applyDelta(this.velocity * elapsed);
      this.velocity *= Math.pow(MOMENTUM_FRICTION, elapsed / 16);
      if (Math.abs(this.velocity) < MOMENTUM_MIN_VELOCITY) return;
      this.momentumFrame = requestAnimationFrame(frame);
    };
    this.momentumFrame = requestAnimationFrame(frame);
  }

  private stopMomentum(): void {
    if (this.momentumFrame !== null) {
      cancelAnimationFrame(this.momentumFrame);
      this.momentumFrame = null;
    }
  }

  private updateBottomButton(): void {
    const buffer = this.terminal.buffer.active;
    const show =
      (buffer.type === 'normal' && buffer.viewportY < buffer.baseY) ||
      (this.options.isPanned?.() ?? false);
    this.bottomButton.classList.toggle(TOUCH_SCROLL_BOTTOM_VISIBLE_CLASS, show);
  }
}
