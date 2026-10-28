// @vitest-environment node
// The "What broke the tests?" sheet is monochrome. Its colours come from the cockpit tokens only, so it follows
// light and dark mode, and every rule sits under the cockpit so it cannot leak onto other pages.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./cockpit-whatbrokeit.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');

/** Every custom property defined anywhere in the styles folder. */
function definedTokens(): Set<string> {
  const dir = fileURLToPath(new URL('.', import.meta.url));
  const names = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.css')) continue;
    const text = readFileSync(`${dir}${file}`, 'utf8');
    for (const match of text.matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) names.add(match[1]);
  }
  return names;
}

describe('what broke it sheet styles', () => {
  it('uses no colour literals (hex, colour functions or named colours)', () => {
    expect(bare).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(bare).not.toMatch(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
    expect(bare).not.toMatch(/:[^;{}]*\b(black|white|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|navy|teal|maroon|lime|aqua|olive)\b/i);
  });

  it('only reads custom properties that exist', () => {
    const tokens = definedTokens();
    const used = [...bare.matchAll(/var\((--[a-zA-Z0-9-]+)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const name of used) expect(tokens.has(name), name).toBe(true);
  });

  it('scopes every selector under the cockpit', () => {
    const selectors = [...bare.matchAll(/([^{}]+)\{/g)]
      .map((m) => m[1].trim())
      .filter((s) => !s.startsWith('@') && !/^(from|to|\d+%)$/.test(s))
      .flatMap((s) => s.split(',').map((part) => part.trim()));
    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit \.ck-(sheet--wb|wb-)/);
  });

  it('has every class the sheet needs', () => {
    for (const name of ['ck-sheet--wb', 'ck-wb-rows', 'ck-wb-row', 'ck-wb-picker', 'ck-wb-warn', 'ck-wb-error', 'ck-wb-bar', 'ck-wb-muted']) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('stops the progress bar moving when the person asks for reduced motion', () => {
    const start = css.indexOf('@media (prefers-reduced-motion: reduce)');
    expect(start).toBeGreaterThan(-1);
    expect(css.slice(start, css.indexOf('}\n}', start))).toMatch(/animation:\s*none/);
  });
});
