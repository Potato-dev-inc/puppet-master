// @vitest-environment node
// The Review Desk styles: monochrome (grayscale cockpit tokens only, no colour literals), every rule under the
// cockpit, only tokens that exist, and every class the desk renders is styled.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./review-desk.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
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

describe('Review Desk styles', () => {
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
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit \.ck-rd/);
  });

  it('styles every class the desk renders', () => {
    for (const name of [
      'ck-rd',
      'ck-rd-head',
      'ck-rd-heading',
      'ck-rd-counts',
      'ck-rd-btn',
      'ck-rd-link',
      'ck-rd-body',
      'ck-rd-list',
      'ck-rd-group',
      'ck-rd-row',
      'ck-rd-line',
      'ck-rd-glyph',
      'ck-rd-title',
      'ck-rd-agent',
      'ck-rd-sub',
      'ck-rd-reason',
      'ck-rd-files',
      'ck-rd-toast',
      'ck-rd-notice',
      'ck-rd-note',
      'ck-rd-empty',
      'ck-rd-detail',
      'ck-rd-keys',
    ]) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('marks risk by weight, not colour: the high-risk title is bolder', () => {
    expect(bare).toMatch(/\.ck-rd-row\[data-level='high'\] \.ck-rd-title \{[^}]*font-weight:\s*600/);
  });
});
