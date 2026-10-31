// With two or more picks the Send button fell below the bottom of the (fixed size) Point box window. The footer is pinned
// to the box's bottom edge and the pick list is the part that shrinks and scrolls.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'point-box.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
const rule = (selector: string): string => {
  const m = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((x) => x[1].trim() === selector);
  if (!m) throw new Error(`no rule for ${selector}`);
  return m[2];
};

describe('Point box layout', () => {
  it('pins the footer so Send stays in view', () => {
    const foot = rule('.point-box-foot');
    expect(foot).toMatch(/position:\s*sticky/);
    expect(foot).toMatch(/bottom:/);
    expect(foot).toMatch(/flex:\s*none/);
  });

  it('caps the pick list and scrolls it, so a third pick cannot grow the form past the window', () => {
    const picks = rule('.point-box-picks');
    expect(picks).toMatch(/max-height:\s*1\d\dpx/);
    expect(picks).toMatch(/overflow-y:\s*auto/);
  });
});
