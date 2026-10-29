// @vitest-environment node
// Collision alerts sit in the Inbox and in the chat banner. Their colours come from the cockpit tokens only, so they
// follow light and dark mode, and every rule sits under the cockpit so it cannot leak onto other pages.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./cockpit-collision.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
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

/** The body of the first rule whose selector is exactly `selector` (comments already stripped from `bare`). */
function ruleBody(selector: string): string {
  const start = bare.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThan(-1);
  return bare.slice(start, bare.indexOf('}', start));
}

describe('collision alert styles', () => {
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
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit \.ck-collision/);
  });

  it('has every class the card, the quiet group and the banner need', () => {
    for (const name of [
      'ck-collision-card',
      'ck-collision-card-head',
      'ck-collision-glyph',
      'ck-collision-title',
      'ck-collision-place',
      'ck-collision-detail',
      'ck-collision-files',
      'ck-collision-advice',
      'ck-collision-actions',
      'ck-collision-footer',
      'ck-collision-quiet',
      'ck-collision-banner',
    ]) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('keeps a same-file card muted: no bold title and no fill', () => {
    const title = ruleBody('.cockpit .ck-collision-card.ck-collision-quiet .ck-collision-title');
    expect(title).toMatch(/font-weight:\s*500/);
    expect(title).not.toMatch(/font-weight:\s*600/);
    expect(ruleBody('.cockpit .ck-collision-card.ck-collision-quiet')).not.toMatch(/background/);
  });

  it('draws the banner as a flat row with a separator, not a box', () => {
    const body = ruleBody('.cockpit .ck-collision-banner');
    expect(body).toMatch(/border-bottom:\s*1px solid var\(--sep\)/);
    expect(body).not.toMatch(/border-radius/);
    expect(body).not.toMatch(/box-shadow/);
  });

  it('marks the glyph as an outline, not a fill', () => {
    const body = ruleBody('.cockpit .ck-collision-glyph');
    expect(body).toMatch(/box-shadow:\s*inset 0 0 0 1px/);
    expect(body).not.toMatch(/background/);
  });
});
