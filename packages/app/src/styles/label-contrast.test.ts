// Small text in --label-3 (hints, "waiting 9s", "Low risk") must reach WCAG AA, 4.5:1, on every
// surface it sits on, in both themes. The CSS files are read as text so the test follows the tokens.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

type RGB = [number, number, number];

const hex = (h: string): RGB => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)) as RGB;
const lin = (c: number) => {
  const v = c / 255;
  return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
};
const luminance = ([r, g, b]: RGB) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
const blend = (fg: RGB, alpha: number, bg: RGB): RGB => fg.map((v, i) => v * alpha + bg[i] * (1 - alpha)) as RGB;
export function contrast(a: RGB, b: RGB): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** The label colour, alpha and the grounds each theme draws it on (from each file's --bg tokens). */
const SURFACES: Record<string, { dark: RGB[]; light: RGB[] }> = {
  'cockpit.css': { dark: ['#0a0a0b', '#111112', '#18181a'].map(hex), light: ['#ffffff', '#f5f5f7'].map(hex) },
  'phone.css': { dark: ['#0a0a0b', '#111112', '#18181a'].map(hex), light: ['#ffffff', '#f5f5f7'].map(hex) },
  'askbar.css': { dark: ['#0a0a0b', '#111112', '#18181a'].map(hex), light: ['#ffffff', '#f5f5f7'].map(hex) },
  'settings.css': { dark: ['#0a0a0b', '#111112', '#18181a'].map(hex), light: ['#ffffff', '#f5f5f7', '#ececef'].map(hex) },
};
const DARK_FG = hex('#f5f5f7');
const LIGHT_FG = hex('#1d1d1f');

function alphas(file: string, token: string): number[] {
  const css = readFileSync(join(__dirname, file), 'utf8');
  return [...css.matchAll(new RegExp(`${token}:\\s*rgba\\([^,]+,[^,]+,[^,]+,\\s*([0-9.]+)\\)`, 'g'))].map((m) => Number(m[1]));
}

describe('--label-3 contrast', () => {
  for (const [file, grounds] of Object.entries(SURFACES)) {
    it(`${file}: at least 4.5:1 in dark and light`, () => {
      const [dark, light] = alphas(file, '--label-3');
      expect(dark, 'dark token found').toBeGreaterThan(0);
      expect(light, 'light token found').toBeGreaterThan(0);
      const ratios = {
        dark: grounds.dark.map((bg) => contrast(blend(DARK_FG, dark, bg), bg)),
        light: grounds.light.map((bg) => contrast(blend(LIGHT_FG, light, bg), bg)),
      };
      // The numbers go into docs/features/review-fixes.md.
      console.log(file, 'label-3 alpha', { dark, light }, 'ratios', JSON.stringify({ dark: ratios.dark.map((r) => +r.toFixed(2)), light: ratios.light.map((r) => +r.toFixed(2)) }));
      for (const r of [...ratios.dark, ...ratios.light]) expect(r).toBeGreaterThanOrEqual(4.5);
    });

    it(`${file}: label-3 stays quieter than label-2`, () => {
      const [d2, l2] = alphas(file, '--label-2');
      const [d3, l3] = alphas(file, '--label-3');
      expect(d3).toBeLessThan(d2);
      expect(l3).toBeLessThan(l2);
    });
  }

  it('the old values failed (control for the check itself)', () => {
    const old = contrast(blend(DARK_FG, 0.4, hex('#0a0a0b')), hex('#0a0a0b'));
    expect(old).toBeLessThan(4.5);
    const oldLight = contrast(blend(LIGHT_FG, 0.42, hex('#ffffff')), hex('#ffffff'));
    expect(oldLight).toBeLessThan(4.5);
  });
});
