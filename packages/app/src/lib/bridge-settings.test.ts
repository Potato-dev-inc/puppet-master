import { describe, expect, it } from 'vitest';
import { clampMobileInputDelayMs } from './bridge-settings';

describe('clampMobileInputDelayMs', () => {
  it('allows zero to disable mobile input buffering', () => {
    expect(clampMobileInputDelayMs(0)).toBe(0);
    expect(clampMobileInputDelayMs(-10)).toBe(0);
  });

  it('clamps nonzero values to the supported custom range', () => {
    expect(clampMobileInputDelayMs(1)).toBe(50);
    expect(clampMobileInputDelayMs(1250)).toBe(1000);
    expect(clampMobileInputDelayMs(275.4)).toBe(275);
  });

  it('uses the default for nonnumeric values', () => {
    expect(clampMobileInputDelayMs(undefined)).toBe(250);
  });
});
