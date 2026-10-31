// The Tests list of a Night Shift task and the sidebar chip's popover once shared the class `ck-night-pop`. The chip's rule
// (bottom: 100%, left/right 0) loaded last and pulled the Tests list above the sheet's top edge, where its Use and close
// buttons could not be pressed. Each popover has its own class, and the Tests list opens under its row.
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const dir = dirname(fileURLToPath(import.meta.url));
const rules = (file: string) => {
  const css = readFileSync(join(dir, file), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({ file, selector: m[1].trim(), body: m[2] }));
};

describe('Night Shift popovers keep their own styles', () => {
  const all = readdirSync(dir)
    .filter((f) => f.endsWith('.css'))
    .flatMap(rules);

  it('styles the Tests list with exactly one rule, and that rule never opens it upward', () => {
    const popRules = all.filter((r) => r.selector.split(',').some((s) => /\.ck-night-pop$/.test(s.trim())));
    expect(popRules.map((r) => r.file)).toEqual(['night.css']);
    expect(popRules[0].body).not.toMatch(/\bbottom\s*:\s*calc\(100%/);
    expect(popRules[0].body).toMatch(/top\s*:\s*auto/);
  });

  it('the sidebar chip popover has its own class', () => {
    expect(all.some((r) => r.file === 'night-chip.css' && /\.ck-night-chip-pop$/.test(r.selector))).toBe(true);
    const chip = readFileSync(join(dir, '../components/cockpit/night/NightChip.tsx'), 'utf8');
    expect(chip).toContain('ck-night-chip-pop');
    expect(chip).not.toMatch(/"ck-night-pop[ "-]/);
  });
});
