import { describe, expect, it, afterEach, beforeEach } from 'vitest';
import {
  containerFitDimensions,
  getKeyboardObscuredPx,
  isKeyboardLikelyOpen,
  measureLayoutWidth,
} from './layout-viewport';

describe('layout-viewport', () => {
  const originalVisualViewport = window.visualViewport;

  beforeEach(() => {
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 800 });
    Object.defineProperty(document.documentElement, 'clientWidth', {
      configurable: true,
      value: 390,
    });
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 390 });
  });

  afterEach(() => {
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: originalVisualViewport,
    });
    document.documentElement.style.removeProperty('--pm-stable-layout-width');
  });

  it('detects keyboard obscured height', () => {
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: { height: 500, offsetTop: 0, width: 390 },
    });
    expect(getKeyboardObscuredPx()).toBe(300);
    expect(isKeyboardLikelyOpen()).toBe(true);
  });

  it('locks fit width while keyboard is open', () => {
    document.documentElement.style.setProperty('--pm-stable-layout-width', '390px');
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: { height: 500, offsetTop: 0, width: 320 },
    });
    const viewport = document.createElement('div');
    Object.defineProperty(viewport, 'clientWidth', { configurable: true, value: 320 });
    Object.defineProperty(viewport, 'clientHeight', { configurable: true, value: 420 });

    expect(containerFitDimensions(viewport)).toEqual({ width: 390, height: 420 });
  });

  it('uses live width when keyboard is closed', () => {
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: { height: 800, offsetTop: 0, width: 390 },
    });
    const viewport = document.createElement('div');
    Object.defineProperty(viewport, 'clientWidth', { configurable: true, value: 385 });
    Object.defineProperty(viewport, 'clientHeight', { configurable: true, value: 700 });

    expect(containerFitDimensions(viewport)).toEqual({ width: 385, height: 700 });
    expect(measureLayoutWidth()).toBe(390);
  });
});
