// @vitest-environment node
// The Scores page is a dense grey table. Its colours come from the cockpit tokens only, so it follows light and
// dark mode, and every rule sits under the cockpit so it cannot leak onto other pages.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./cockpit-scores.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
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

describe('scores page styles', () => {
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
      .filter((s) => !s.startsWith('@'))
      .flatMap((s) => s.split(',').map((part) => part.trim()));
    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit \.ck-scores/);
  });

  it('has every class the page needs', () => {
    for (const name of [
      'ck-scores',
      'ck-scores-controls',
      'ck-scores-table',
      'is-early',
      'ck-scores-tag',
      'ck-scores-ahead',
      'ck-scores-notenough',
      'ck-scores-how',
      'ck-scores-notes',
    ]) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('marks a chip as an outline, not a fill', () => {
    const start = css.indexOf('.cockpit .ck-scores-ahead {');
    expect(start).toBeGreaterThan(-1);
    const body = css.slice(start, css.indexOf('}', start));
    expect(body).toMatch(/box-shadow:\s*inset 0 0 0 1px/);
    expect(body).not.toMatch(/background/);
  });
});
