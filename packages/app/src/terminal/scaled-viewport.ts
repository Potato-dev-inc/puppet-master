import type { Disposable } from './types';
import {
  containerFitDimensions,
  getKeyboardObscuredPx,
  isKeyboardLikelyOpen,
} from '../lib/layout-viewport';

/** Dispatched when desktop PTY authority width changes (sidebar / mobile viewport). */
export const TERMINAL_AUTHORITY_CHANGED_EVENT = 'pm:terminal-authority-changed';

export const TERMINAL_SCALE_VIEWPORT_CLASS = 'terminal-scale-viewport';
export const TERMINAL_SCALE_STAGE_CLASS = 'terminal-scale-stage';

export interface ContainerFitScaleOptions {
  /** Do not upscale past 1 (content smaller than container stays 1:1). */
  maxScale?: number;
  minScale?: number;
  /** Ignore container height as a limiting axis. Used while mobile keyboards are open. */
  ignoreHeight?: boolean;
}

export interface ContainerFitTransformOptions extends ContainerFitScaleOptions {
  pushUp?: boolean;
  maxPushUpPx?: number;
}

export interface ContainerFitTransform {
  scale: number;
  translateY: number;
}

/**
 * Uniform scale so the full terminal canvas fits inside its container
 * (width and height share the same ratio).
 */
export function computeContainerFitScale(
  contentWidth: number,
  contentHeight: number,
  containerWidth: number,
  containerHeight: number,
  options: ContainerFitScaleOptions = {},
): number {
  const maxScale = options.maxScale ?? 1;
  const minScale = options.minScale ?? 0.05;
  if (
    contentWidth <= 0 ||
    contentHeight <= 0 ||
    containerWidth <= 0 ||
    containerHeight <= 0
  ) {
    return 1;
  }
  const widthRatio = containerWidth / contentWidth;
  const heightRatio = options.ignoreHeight ? Number.POSITIVE_INFINITY : containerHeight / contentHeight;
  const ratio = Math.min(widthRatio, heightRatio);
  return Math.min(maxScale, Math.max(minScale, ratio));
}

export function computeContainerFitTransform(
  contentWidth: number,
  contentHeight: number,
  containerWidth: number,
  containerHeight: number,
  options: ContainerFitTransformOptions = {},
): ContainerFitTransform {
  const scale = computeContainerFitScale(
    contentWidth,
    contentHeight,
    containerWidth,
    containerHeight,
    options,
  );

  if (!options.pushUp || contentHeight <= 0 || containerHeight <= 0) {
    return { scale, translateY: 0 };
  }

  const overflowY = Math.max(0, contentHeight * scale - containerHeight);
  const maxPushUpPx = options.maxPushUpPx ?? overflowY;
  return {
    scale,
    translateY: -Math.min(overflowY, Math.max(0, maxPushUpPx)),
  };
}

/**
 * Vertical offset for a stage that is never scaled: 0 when it fits, otherwise a negative
 * offset that keeps the bottom edge (the prompt) on the bottom of the container and crops the top.
 */
export function computeBottomAnchoredTranslateY(
  contentHeight: number,
  containerHeight: number,
): number {
  if (contentHeight <= 0 || containerHeight <= 0) return 0;
  return Math.min(0, containerHeight - contentHeight);
}

/**
 * Vertical offset that keeps the cursor line visible: the top of the grid stays on screen and the
 * bottom is cut, unless that would hide the cursor (then the stage slides up just far enough).
 * `cursorBottomPx` is measured from the top of the content, already clamped to the content height.
 * Result is within `[containerHeight - contentHeight, 0]`.
 */
export function computeCursorAnchoredTranslateY(
  contentHeight: number,
  containerHeight: number,
  cursorBottomPx: number,
): number {
  if (contentHeight <= 0 || containerHeight <= 0) return 0;
  if (contentHeight <= containerHeight) return 0;
  const bottom = Math.min(Math.max(cursorBottomPx, 0), contentHeight);
  const wanted = Math.min(0, containerHeight - bottom);
  return Math.max(wanted, containerHeight - contentHeight);
}

/** Allowed range for a stage's translateY: `[containerHeight - contentHeight, 0]` (0 when it fits). */
export function translateYBounds(
  contentHeight: number,
  containerHeight: number,
): { min: number; max: number } {
  if (contentHeight <= 0 || containerHeight <= 0) return { min: 0, max: 0 };
  return { min: Math.min(0, containerHeight - contentHeight), max: 0 };
}

/** translateY clamped to `translateYBounds`. Non-finite values clamp to the top (0). */
export function clampTranslateY(
  translateY: number,
  contentHeight: number,
  containerHeight: number,
): number {
  const { min, max } = translateYBounds(contentHeight, containerHeight);
  if (!Number.isFinite(translateY)) return max;
  return Math.min(Math.max(translateY, min), max);
}

/**
 * Applies a pan delta (px, positive = content moves down to reveal the top) to an absolute
 * translateY, clamped to the allowed range. Returns the new translateY and the part of the delta
 * that was not consumed (for the caller to scroll with instead).
 */
export function applyPanDelta(
  translateY: number,
  dyPx: number,
  contentHeight: number,
  containerHeight: number,
): { translateY: number; leftoverPx: number } {
  const current = clampTranslateY(translateY, contentHeight, containerHeight);
  if (!Number.isFinite(dyPx) || dyPx === 0) {
    return { translateY: current, leftoverPx: 0 };
  }
  const next = clampTranslateY(current + dyPx, contentHeight, containerHeight);
  return { translateY: next, leftoverPx: dyPx - (next - current) };
}

/**
 * Uniform scale for a never-scaled stage that is wider than its container: shrinks it to the
 * container width. Never upscales; 1 when the content fits or a size is unknown.
 */
export function computeNoScaleWidthScale(contentWidth: number, containerWidth: number): number {
  if (contentWidth <= 0 || containerWidth <= 0) return 1;
  if (contentWidth <= containerWidth) return 1;
  return containerWidth / contentWidth;
}

function measureTerminalContent(stage: HTMLElement): { width: number; height: number } {
  const term = stage.querySelector('.xterm');
  if (!(term instanceof HTMLElement)) {
    return { width: 0, height: 0 };
  }
  const screen = term.querySelector('.xterm-screen');
  const width = screen instanceof HTMLElement
    ? Math.max(screen.offsetWidth, screen.scrollWidth, term.offsetWidth)
    : Math.max(term.offsetWidth, term.scrollWidth);
  const height = screen instanceof HTMLElement
    ? Math.max(screen.offsetHeight, screen.scrollHeight, term.offsetHeight)
    : Math.max(term.offsetHeight, term.scrollHeight);
  return { width, height };
}

export function isMobileKeyboardTransitionActive(viewport: HTMLElement): boolean {
  if (isKeyboardLikelyOpen()) return true;

  const host = viewport.closest('.terminal-host--mobile-input');
  if (!(host instanceof HTMLElement)) return false;
  if (host.classList.contains('terminal-host--mobile-input-engaged')) return true;

  const activeElement = document.activeElement;
  return (
    activeElement instanceof HTMLElement &&
    activeElement.matches('[data-mobile-terminal-input="true"]')
  );
}

/**
 * Scales a mirror terminal stage to fit its viewport using CSS transform.
 * The PTY grid stays at desktop cols/rows; only the presentation shrinks.
 */
export class TerminalScaleController implements Disposable {
  private readonly viewport: HTMLElement;
  private readonly stage: HTMLElement;
  private resizeObserver: ResizeObserver | null = null;
  private fitFrame: number | null = null;
  private readonly onAuthorityChange: () => void;

  /** False: always fit width and height, never push the stage up for the keyboard. */
  private readonly keyboardPushUp: boolean;

  /** Content size from the caller (grid arithmetic) instead of measuring the rendered stage. */
  private readonly measureContent?: () => { width: number; height: number } | null;

  /**
   * Never scale the stage by height: the stage is shifted vertically so the cursor line stays
   * visible (top kept, bottom cut when possible). A stage wider than the container is shrunk to
   * the container width. Width is otherwise left-aligned.
   */
  private readonly noScale: boolean;

  /** noScale only: cursor bottom (px from the content top, clamped), or null to bottom-anchor. */
  private readonly measureCursorBottom?: () => number | null;

  /**
   * noScale only: user's absolute translateY while panned away from the default anchor, or null
   * when unpanned (the anchor then follows the cursor). Re-clamped on every fit.
   */
  private pannedTranslateY: number | null = null;

  constructor(
    viewport: HTMLElement,
    stage: HTMLElement,
    options: {
      keyboardPushUp?: boolean;
      measureContent?: () => { width: number; height: number } | null;
      measureCursorBottom?: () => number | null;
      noScale?: boolean;
    } = {},
  ) {
    this.viewport = viewport;
    this.stage = stage;
    this.keyboardPushUp = options.keyboardPushUp ?? true;
    this.measureContent = options.measureContent;
    this.measureCursorBottom = options.measureCursorBottom;
    this.noScale = options.noScale ?? false;
    this.onAuthorityChange = () => {
      this.scheduleFit();
    };

    this.resizeObserver = new ResizeObserver(() => {
      this.scheduleFit();
    });
    this.resizeObserver.observe(viewport);
    window.addEventListener(TERMINAL_AUTHORITY_CHANGED_EVENT, this.onAuthorityChange);
    window.visualViewport?.addEventListener('resize', this.onAuthorityChange);
    window.visualViewport?.addEventListener('scroll', this.onAuthorityChange);
    this.scheduleFit();
  }

  scheduleFit(): void {
    if (this.fitFrame !== null) return;
    this.fitFrame = requestAnimationFrame(() => {
      this.fitFrame = null;
      this.fitNow();
    });
  }

  /**
   * Pans the cropped stage by `dyPx` (positive = reveal the top). Applies at once and returns the
   * part of `dyPx` the stage could not take, so the caller can scroll with the rest. Only the
   * noScale mode pans; otherwise nothing is consumed.
   */
  panBy(dyPx: number): number {
    if (!this.noScale) return dyPx;
    const layout = this.noScaleLayout();
    const current = this.pannedTranslateY ?? layout.anchor;
    const { translateY, leftoverPx } = applyPanDelta(
      current,
      dyPx,
      layout.scaledHeight,
      layout.containerHeight,
    );
    this.pannedTranslateY = translateY === layout.anchor ? null : translateY;
    this.fitNow();
    return leftoverPx;
  }

  /** True while the stage is panned away from its cursor-anchored position. */
  isPanned(): boolean {
    return this.noScale && this.pannedTranslateY !== null;
  }

  /** Returns the stage to its default (cursor-anchored) position. */
  resetPan(): void {
    if (this.pannedTranslateY === null) return;
    this.pannedTranslateY = null;
    this.fitNow();
  }

  /** noScale layout: scale from width, vertical anchor and bounds, all in screen px. */
  private noScaleLayout(): {
    scale: number;
    contentWidth: number;
    contentHeight: number;
    scaledHeight: number;
    containerHeight: number;
    anchor: number;
  } {
    const { width: contentWidth, height: contentHeight } =
      this.measureContent?.() ?? measureTerminalContent(this.stage);
    const { width: containerWidth, height: containerHeight } = containerFitDimensions(this.viewport);
    const scale = computeNoScaleWidthScale(contentWidth, containerWidth);
    const scaledHeight = contentHeight * scale;
    const cursorBottom = this.measureCursorBottom?.() ?? null;
    const anchor =
      cursorBottom === null
        ? computeBottomAnchoredTranslateY(scaledHeight, containerHeight)
        : computeCursorAnchoredTranslateY(scaledHeight, containerHeight, cursorBottom * scale);
    return { scale, contentWidth, contentHeight, scaledHeight, containerHeight, anchor };
  }

  fitNow(): void {
    if (this.noScale) {
      const layout = this.noScaleLayout();
      if (this.pannedTranslateY !== null) {
        // Re-clamp on every fit: the pan resets by itself once it matches the default anchor.
        this.pannedTranslateY = clampTranslateY(
          this.pannedTranslateY,
          layout.scaledHeight,
          layout.containerHeight,
        );
        if (this.pannedTranslateY === layout.anchor) this.pannedTranslateY = null;
      }
      this.applyStage(
        layout.contentWidth,
        layout.contentHeight,
        layout.scale,
        this.pannedTranslateY ?? layout.anchor,
      );
      return;
    }
    const { width: contentWidth, height: contentHeight } =
      this.measureContent?.() ?? measureTerminalContent(this.stage);
    const { width: containerWidth, height: containerHeight } = containerFitDimensions(this.viewport);
    const keyboardOpen = this.keyboardPushUp && isMobileKeyboardTransitionActive(this.viewport);
    const { scale, translateY } = computeContainerFitTransform(
      contentWidth,
      contentHeight,
      containerWidth,
      containerHeight,
      {
        ignoreHeight: keyboardOpen,
        pushUp: keyboardOpen,
        maxPushUpPx: getKeyboardObscuredPx(),
      },
    );
    this.applyStage(contentWidth, contentHeight, scale, translateY);
  }

  private applyStage(
    contentWidth: number,
    contentHeight: number,
    scale: number,
    translateY: number,
  ): void {
    if (contentWidth > 0 && contentHeight > 0) {
      this.stage.style.width = `${contentWidth}px`;
      this.stage.style.height = `${contentHeight}px`;
    }

    this.stage.style.transformOrigin = 'top left';
    const transforms = [];
    if (translateY !== 0) {
      transforms.push(`translateY(${translateY}px)`);
    }
    if (scale !== 1) {
      transforms.push(`scale(${scale})`);
    }
    this.stage.style.transform = transforms.join(' ');
    this.viewport.style.setProperty('--pm-terminal-scale', String(scale));
    this.viewport.style.setProperty('--pm-terminal-translate-y', `${translateY}px`);
    this.viewport.style.setProperty('--pm-terminal-content-width', `${contentWidth}px`);
    this.viewport.style.setProperty('--pm-terminal-content-height', `${contentHeight}px`);
  }

  dispose(): void {
    if (this.fitFrame !== null) {
      cancelAnimationFrame(this.fitFrame);
      this.fitFrame = null;
    }
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    window.removeEventListener(TERMINAL_AUTHORITY_CHANGED_EVENT, this.onAuthorityChange);
    window.visualViewport?.removeEventListener('resize', this.onAuthorityChange);
    window.visualViewport?.removeEventListener('scroll', this.onAuthorityChange);
    this.stage.style.transform = '';
    this.stage.style.width = '';
    this.stage.style.height = '';
    this.viewport.style.removeProperty('--pm-terminal-scale');
    this.viewport.style.removeProperty('--pm-terminal-translate-y');
    this.viewport.style.removeProperty('--pm-terminal-content-width');
    this.viewport.style.removeProperty('--pm-terminal-content-height');
  }
}

export function createTerminalScaleMount(container: HTMLElement): {
  viewport: HTMLElement;
  stage: HTMLElement;
} {
  container.classList.add('terminal-host--mirror-scale');
  const viewport = document.createElement('div');
  viewport.className = TERMINAL_SCALE_VIEWPORT_CLASS;
  const stage = document.createElement('div');
  stage.className = TERMINAL_SCALE_STAGE_CLASS;
  viewport.appendChild(stage);
  container.appendChild(viewport);
  return { viewport, stage };
}
