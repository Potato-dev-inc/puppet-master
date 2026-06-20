import { describe, expect, it } from 'vitest';
import {
  computeContainerFitScale,
  computeContainerFitTransform,
  isMobileKeyboardTransitionActive,
} from './scaled-viewport';

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
