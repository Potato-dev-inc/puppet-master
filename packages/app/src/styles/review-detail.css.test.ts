// @vitest-environment node
// The Review Desk detail uses the cockpit tokens only, so it follows light and dark mode and carries no colour of
// its own. Every rule sits under the cockpit so it cannot leak onto other pages.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./review-detail.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
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

describe('review detail styles', () => {
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
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit /);
  });

  it('has every class the detail and its sheets need', () => {
    for (const name of [
      'ck-rd-detail',
      'ck-rd-detail-title',
      'ck-rd-detail-reasons',
      'ck-rd-detail-facts',
      'ck-rd-detail-files',
      'ck-rd-detail-file',
      'ck-rd-detail-diff',
      'ck-rd-detail-add',
      'ck-rd-detail-del',
      'ck-rd-detail-hunk',
      'ck-rd-detail-actions',
      'ck-rd-detail-key',
      'ck-rd-sheet',
      'ck-rd-sheet-warn',
      'ck-rd-sheet-error',
      'ck-rd-fix-preview',
      'ck-rd-fix-note',
    ]) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('keeps the diff in a monospace face and the sheets at a fixed width', () => {
    const start = css.indexOf('.cockpit .ck-rd-detail-diff {');
    expect(start).toBeGreaterThan(-1);
    expect(css.slice(start, css.indexOf('}', start))).toMatch(/font-family:\s*var\(--font-mono\)/);
    expect(css).toMatch(/\.cockpit \.ck-sheet\.ck-rd-sheet \{\s*width:/);
  });
});
