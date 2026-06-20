import { describe, expect, it } from 'vitest';
import {
  SIDEBAR_WIDTH_PRESETS,
  clampSidebarWidth,
  isSidebarWidthPreset,
} from './settings';

describe('sidebar width helpers', () => {
  it('clamps to supported range', () => {
    expect(clampSidebarWidth(250)).toBe(300);
    expect(clampSidebarWidth(900)).toBe(800);
    expect(clampSidebarWidth(400.7)).toBe(401);
  });

  it('recognizes preset widths', () => {
    for (const preset of SIDEBAR_WIDTH_PRESETS) {
      expect(isSidebarWidthPreset(preset)).toBe(true);
    }
    expect(isSidebarWidthPreset(425)).toBe(false);
  });
});
