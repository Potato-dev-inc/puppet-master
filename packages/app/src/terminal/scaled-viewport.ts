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

  constructor(viewport: HTMLElement, stage: HTMLElement) {
    this.viewport = viewport;
    this.stage = stage;
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

  fitNow(): void {
    const { width: contentWidth, height: contentHeight } = measureTerminalContent(this.stage);
    const { width: containerWidth, height: containerHeight } = containerFitDimensions(this.viewport);
    const keyboardOpen = isMobileKeyboardTransitionActive(this.viewport);
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
