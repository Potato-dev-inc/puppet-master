// @vitest-environment node
// "Check my setup" styles live in settings.css. They use Settings tokens only (no colour literals), every token they
// read is defined on `.settings`, and the fix button is aligned to the right of its row.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./settings.css', import.meta.url), 'utf8');
const block = css.slice(css.indexOf('Check my setup (Settings > Agents)'));
const rootBlock = css.match(/^\.settings \{([\s\S]*?)\n\}/m)?.[1] ?? '';

describe('Check my setup styles', () => {
  it('uses no colour literals', () => {
    expect(block).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(block).not.toMatch(/\b(rgb|rgba|hsl|hsla)\(/);
  });

  it('reads only tokens that Settings defines', () => {
    const used = [...block.matchAll(/var\((--[a-z0-9-]+)\)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const token of new Set(used)) {
      expect(rootBlock).toContain(`${token}:`);
    }
  });

  it('puts the header buttons on the right', () => {
    expect(block).toMatch(/\.settings \.st-setupcheck-head\s*\{[^}]*justify-content:\s*flex-end/);
  });
});
