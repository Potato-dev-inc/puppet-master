/** Match {@link mobile-input-guard} keyboard detection threshold. */
const MOBILE_KEYBOARD_MIN_OBSCURED_PX = 100;

export const STABLE_LAYOUT_WIDTH_VAR = '--pm-stable-layout-width';

export function getKeyboardObscuredPx(): number {
  if (typeof window === 'undefined') return 0;
  const vv = window.visualViewport;
  if (!vv) return 0;
  return Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
}

export function isKeyboardLikelyOpen(): boolean {
  return getKeyboardObscuredPx() >= MOBILE_KEYBOARD_MIN_OBSCURED_PX;
}

/** Full layout width before the software keyboard squeezes the visual viewport. */
export function measureLayoutWidth(): number {
  if (typeof window === 'undefined') return 0;
  const vv = window.visualViewport;
  return Math.max(
    document.documentElement.clientWidth,
    window.innerWidth,
    vv?.width ?? 0,
  );
}

export function readStableLayoutWidthPx(): number {
  if (typeof document === 'undefined') return 0;
  const raw = getComputedStyle(document.documentElement)
    .getPropertyValue(STABLE_LAYOUT_WIDTH_VAR)
    .trim();
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : measureLayoutWidth();
}

/**
 * Container size for terminal fit/scale: width stays locked while the keyboard
 * is open; height follows the live viewport.
 */
export function containerFitDimensions(viewportEl: HTMLElement): {
  width: number;
  height: number;
} {
  const height = viewportEl.clientHeight;
  const liveWidth = viewportEl.clientWidth;
  if (isKeyboardLikelyOpen()) {
    return {
      width: readStableLayoutWidthPx(),
      height,
    };
  }
  return {
    width: liveWidth,
    height,
  };
}
