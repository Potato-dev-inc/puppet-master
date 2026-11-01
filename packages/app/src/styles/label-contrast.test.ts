// Colour tokens, read from the CSS as text so the test follows the tokens:
//  - small text in --label-2 and --label-3 (hints, "waiting 9s", "Low risk") reaches WCAG AA, 4.5:1, on every surface it
//    sits on, in both themes;
//  - the dark theme has no pure-black surface (charcoal steps that get lighter with elevation);
//  - the accent (Potato orange) is readable as text on those surfaces, its fill carries --accent-fg at 4.5:1, and its
//    1-2 px markers reach 3:1 (non-text contrast).
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

/** Token files and the names they use for the grounds (the Settings window calls them --window and --list). */
const FILES: Record<string, { ground: string[]; float: string }> = {
  'cockpit.css': { ground: ['--bg', '--bg-side', '--bg-raised'], float: '--bg-float' },
  'phone.css': { ground: ['--bg', '--bg-side', '--bg-raised'], float: '--bg-float' },
  'askbar.css': { ground: ['--bg', '--bg-side', '--bg-raised'], float: '--bg-float' },
  'settings.css': { ground: ['--window', '--bg-side', '--list'], float: '--float-bg' },
  'point-box.css': { ground: ['--bg', '--bg-raised'], float: '--bg-float' },
};

const read = (file: string) => readFileSync(join(__dirname, file), 'utf8');
/** Every value a custom property is given in the file, in order: [dark, light]. */
function values(file: string, token: string): string[] {
  const css = read(file);
  return [...css.matchAll(new RegExp(`^\\s*${token}:\\s*([^;]+);`, 'gm'))].map((m) => m[1].trim());
}
const rgba = (v: string): { rgb: RGB; a: number } => {
  const m = v.match(/rgba?\(\s*(\d+),\s*(\d+),\s*(\d+)(?:,\s*([0-9.]+))?\)/);
  if (!m) throw new Error('not an rgba(): ' + v);
  return { rgb: [Number(m[1]), Number(m[2]), Number(m[3])], a: m[4] === undefined ? 1 : Number(m[4]) };
};
const alphas = (file: string, token: string): number[] => values(file, token).map((v) => rgba(v).a);

/** The grounds of a theme (0 = dark, 1 = light), the float surface drawn over the main ground included. */
function grounds(file: string, theme: 0 | 1): RGB[] {
  const { ground, float } = FILES[file];
  const solid = ground.map((name) => hex(values(file, name)[theme]));
  const f = rgba(values(file, float)[theme]);
  return [...solid, blend(f.rgb, f.a, solid[0])];
}

describe.each(Object.keys(FILES))('%s colour tokens', (file) => {
  const darkFg = hex(values(file, '--label')[0]);
  const lightFg = hex(values(file, '--label')[1]);

  for (const token of ['--label-2', '--label-3']) {
    it(`${token}: at least 4.5:1 on every ground, dark and light`, () => {
      const [dark, light] = alphas(file, token);
      expect(dark, 'dark token found').toBeGreaterThan(0);
      expect(light, 'light token found').toBeGreaterThan(0);
      const ratios = {
        dark: grounds(file, 0).map((bg) => contrast(blend(darkFg, dark, bg), bg)),
        light: grounds(file, 1).map((bg) => contrast(blend(lightFg, light, bg), bg)),
      };
      // The numbers go into docs/design/theme.md.
      console.log(file, token, { dark, light }, JSON.stringify({ dark: ratios.dark.map((r) => +r.toFixed(2)), light: ratios.light.map((r) => +r.toFixed(2)) }));
      for (const r of [...ratios.dark, ...ratios.light]) expect(r).toBeGreaterThanOrEqual(4.5);
    });
  }

  it('label-3 stays quieter than label-2', () => {
    const [d2, l2] = alphas(file, '--label-2');
    const [d3, l3] = alphas(file, '--label-3');
    expect(d3).toBeLessThan(d2);
    expect(l3).toBeLessThan(l2);
  });

  it('the dark theme has no pure-black surface and the grounds get lighter with elevation', () => {
    const solid = FILES[file].ground.map((name) => luminance(hex(values(file, name)[0])));
    // #1b1a19 is about 0.010; the old #0a0a0b was 0.003. Every ground sits above #181716 (0.0087).
    for (const l of solid) expect(l).toBeGreaterThan(0.008);
    for (let i = 1; i < solid.length; i++) expect(solid[i]).toBeGreaterThanOrEqual(solid[i - 1]);
  });

  it('the accent: text 4.5:1 on every ground, --accent-fg 4.5:1 on the fill, the marker 3:1', () => {
    for (const theme of [0, 1] as const) {
      const accent = hex(values(file, '--accent')[theme]);
      const fg = hex(values(file, '--accent-fg')[theme]);
      const line = hex(values(file, '--accent-line')[theme]);
      for (const bg of grounds(file, theme)) {
        expect(contrast(accent, bg), `accent text, theme ${theme}`).toBeGreaterThanOrEqual(4.5);
        expect(contrast(line, bg), `accent marker, theme ${theme}`).toBeGreaterThanOrEqual(3);
      }
      expect(contrast(fg, accent), `accent-fg on accent, theme ${theme}`).toBeGreaterThanOrEqual(4.5);
      for (const name of ['--accent-hover', '--accent-press']) {
        expect(contrast(fg, hex(values(file, name)[theme])), `${name}, theme ${theme}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});

describe('the old values (control for the check itself)', () => {
  it('failed 4.5:1 for label-3 at the old alphas', () => {
    const black = hex('#0a0a0b');
    expect(contrast(blend(hex('#f5f5f7'), 0.4, black), black)).toBeLessThan(4.5);
    const white = hex('#ffffff');
    expect(contrast(blend(hex('#1d1d1f'), 0.42, white), white)).toBeLessThan(4.5);
  });
});
