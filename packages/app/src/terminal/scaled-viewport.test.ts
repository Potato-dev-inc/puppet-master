import { describe, expect, it } from 'vitest';
import {
  applyPanDelta,
  clampTranslateY,
  computeBottomAnchoredTranslateY,
  computeContainerFitScale,
  computeContainerFitTransform,
  computeCursorAnchoredTranslateY,
  computeNoScaleWidthScale,
  isMobileKeyboardTransitionActive,
  translateYBounds,
} from './scaled-viewport';

describe('computeBottomAnchoredTranslateY', () => {
  it('keeps the top at 0 when the content fits', () => {
    expect(computeBottomAnchoredTranslateY(300, 500)).toBe(0);
    expect(computeBottomAnchoredTranslateY(500, 500)).toBe(0);
  });

  it('shifts up by the overflow so the bottom rows stay on screen', () => {
    // 900px of rows in a 500px area: show the bottom 500px.
    expect(computeBottomAnchoredTranslateY(900, 500)).toBe(-400);
  });

  it('returns 0 for invalid measurements', () => {
    expect(computeBottomAnchoredTranslateY(0, 500)).toBe(0);
    expect(computeBottomAnchoredTranslateY(900, 0)).toBe(0);
  });
});

describe('computeContainerFitScale', () => {
  it('fits wide content into a narrow container uniformly', () => {
    const scale = computeContainerFitScale(540, 400, 385, 700);
    expect(scale).toBeCloseTo(385 / 540, 5);
  });

  it('fits tall content when height is the limiting axis', () => {
    const scale = computeContainerFitScale(300, 800, 385, 400);
    expect(scale).toBeCloseTo(400 / 800, 5);
  });

  it('does not upscale when content is smaller than the container', () => {
    expect(computeContainerFitScale(200, 100, 385, 700)).toBe(1);
  });

  it('returns 1 for invalid measurements', () => {
    expect(computeContainerFitScale(0, 400, 385, 700)).toBe(1);
    expect(computeContainerFitScale(540, 400, 0, 700)).toBe(1);
  });

  it('can ignore height when fitting against a keyboard-shrunk viewport', () => {
    const scale = computeContainerFitScale(390, 900, 390, 500, {
      ignoreHeight: true,
    });

    expect(scale).toBe(1);
  });

  it('pushes overflowing keyboard content upward without changing width-fit scale', () => {
    const fit = computeContainerFitTransform(390, 900, 390, 500, {
      ignoreHeight: true,
      pushUp: true,
      maxPushUpPx: 280,
    });

    expect(fit.scale).toBe(1);
    expect(fit.translateY).toBe(-280);
  });

  it('enters keyboard transition mode while hidden mobile input is engaged', () => {
    const host = document.createElement('div');
    host.className = 'terminal-host--mobile-input terminal-host--mobile-input-engaged';
    const viewport = document.createElement('div');
    host.appendChild(viewport);
    document.body.appendChild(host);

    expect(isMobileKeyboardTransitionActive(viewport)).toBe(true);

    host.remove();
  });

  it('enters keyboard transition mode as soon as mobile input is focused', () => {
    const host = document.createElement('div');
    host.className = 'terminal-host--mobile-input';
    const viewport = document.createElement('div');
    const input = document.createElement('textarea');
    input.setAttribute('data-mobile-terminal-input', 'true');
    host.append(viewport, input);
    document.body.appendChild(host);

    input.focus();

    expect(isMobileKeyboardTransitionActive(viewport)).toBe(true);

    host.remove();
  });
});

describe('computeCursorAnchoredTranslateY', () => {
  it('keeps the top at 0 when the content fits', () => {
    expect(computeCursorAnchoredTranslateY(300, 500, 100)).toBe(0);
    expect(computeCursorAnchoredTranslateY(500, 500, 500)).toBe(0);
  });

  it('keeps the top when the cursor is near the top (bottom of the content is cut)', () => {
    // 1000px of rows in a 500px area, cursor row ends at 120px.
    expect(computeCursorAnchoredTranslateY(1000, 500, 120)).toBe(0);
  });

  it('bottom-anchors when the cursor is at the bottom', () => {
    expect(computeCursorAnchoredTranslateY(1000, 500, 1000)).toBe(-500);
  });

  it('slides up just far enough to keep the cursor row in view in between', () => {
    // Cursor row ends at 700px: the bottom of the view (500px) must reach 700px.
    expect(computeCursorAnchoredTranslateY(1000, 500, 700)).toBe(-200);
    // Cursor ends at 900px: the view shows 400px..900px.
    expect(computeCursorAnchoredTranslateY(1000, 500, 900)).toBe(-400);
  });

  it('clamps the cursor position to the content', () => {
    expect(computeCursorAnchoredTranslateY(1000, 500, 5000)).toBe(-500);
    expect(computeCursorAnchoredTranslateY(1000, 500, -20)).toBe(0);
  });

  it('returns 0 for invalid measurements', () => {
    expect(computeCursorAnchoredTranslateY(0, 500, 100)).toBe(0);
    expect(computeCursorAnchoredTranslateY(900, 0, 100)).toBe(0);
  });
});

describe('computeNoScaleWidthScale', () => {
  it('shrinks a grid wider than the container to fit its width', () => {
    expect(computeNoScaleWidthScale(540, 270)).toBeCloseTo(0.5, 5);
  });

  it('stays 1 when the grid fits the width', () => {
    expect(computeNoScaleWidthScale(385, 385)).toBe(1);
    expect(computeNoScaleWidthScale(200, 385)).toBe(1);
  });

  it('stays 1 for invalid measurements', () => {
    expect(computeNoScaleWidthScale(0, 385)).toBe(1);
    expect(computeNoScaleWidthScale(540, 0)).toBe(1);
  });
});

describe('translate bounds and pan', () => {
  it('allows no movement when the content fits', () => {
    expect(translateYBounds(300, 500)).toEqual({ min: 0, max: 0 });
    expect(clampTranslateY(120, 300, 500)).toBe(0);
  });

  it('bounds the translate between the bottom and the top of the content', () => {
    expect(translateYBounds(800, 500)).toEqual({ min: -300, max: 0 });
    expect(clampTranslateY(-400, 800, 500)).toBe(-300);
    expect(clampTranslateY(50, 800, 500)).toBe(0);
    expect(clampTranslateY(-120, 800, 500)).toBe(-120);
  });

  it('pans toward the top and returns the unconsumed part of the delta', () => {
    expect(applyPanDelta(-200, 100, 800, 500)).toEqual({ translateY: -100, leftoverPx: 0 });
    expect(applyPanDelta(-100, 150, 800, 500)).toEqual({ translateY: 0, leftoverPx: 50 });
    expect(applyPanDelta(0, 40, 800, 500)).toEqual({ translateY: 0, leftoverPx: 40 });
  });

  it('pans toward the bottom and returns the rest past the limit', () => {
    expect(applyPanDelta(-100, -250, 800, 500)).toEqual({ translateY: -300, leftoverPx: -50 });
    expect(applyPanDelta(-200, -40, 800, 500)).toEqual({ translateY: -240, leftoverPx: 0 });
  });

  it('takes nothing when the content fits', () => {
    expect(applyPanDelta(0, 40, 300, 500)).toEqual({ translateY: 0, leftoverPx: 40 });
  });
});
