// @vitest-environment node
// The Focus note is monochrome: no hue anywhere in its stylesheet, only cockpit tokens. A red word would pull the
// eye away from the card that actually needs the person.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./focus-note.css', import.meta.url), 'utf8');

describe('the Focus note stylesheet', () => {
  it('uses no coloured hex value (only grays)', () => {
    const hex = [...css.matchAll(/#([0-9a-fA-F]{3,8})\b/g)].map((m) => m[1]);
    for (const h of hex) {
      const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h.slice(0, 6);
      const [r, g, b] = [0, 2, 4].map((i) => full.slice(i, i + 2));
      expect(r === g && g === b, `#${h} is not a gray`).toBe(true);
    }
  });

  it('uses no rgb() or hsl() colour with a hue', () => {
    expect(css).not.toMatch(/\bhsla?\(/);
    for (const m of css.matchAll(/\brgba?\(([^)]*)\)/g)) {
      const [r, g, b] = m[1].split(',').slice(0, 3).map((v) => Number(v.trim()));
      expect(r === g && g === b, `rgb(${m[1]}) is not a gray`).toBe(true);
    }
  });

  it('colours only with cockpit tokens', () => {
    const colours = [...css.matchAll(/(?:^|[\s{;])(?:color|background):\s*([^;]+);/g)].map((m) => m[1].trim());
    expect(colours.length).toBeGreaterThan(0);
    for (const v of colours) {
      if (v === 'none' || v === '0' || v === 'inherit' || v.startsWith('var(--')) continue;
      expect(['transparent', 'currentColor'].includes(v), `unexpected colour value "${v}"`).toBe(true);
    }
  });
});
