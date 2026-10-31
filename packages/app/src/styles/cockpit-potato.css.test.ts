// @vitest-environment node
// Potato Bot styles: grayscale tokens only (no colour literals), every custom property exists, every rule sits under
// the cockpit, your bubbles are filled and the bot's are outlined, and the text is 13 px.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./cockpit-potato.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');

function definedTokens(): Set<string> {
  const dir = fileURLToPath(new URL('.', import.meta.url));
  const names = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.css')) continue;
    for (const match of readFileSync(`${dir}${file}`, 'utf8').matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) names.add(match[1]);
  }
  return names;
}

function ruleBody(selector: string): string {
  const start = bare.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThan(-1);
  return bare.slice(start, bare.indexOf('}', start));
}

describe('Potato Bot styles', () => {
  it('uses no colour literals', () => {
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
    for (const s of selectors) expect(s.startsWith('.cockpit '), s).toBe(true);
  });

  it('fills your bubbles and only outlines the bot bubbles', () => {
    expect(ruleBody('.cockpit .ck-potato-bubble--you')).toMatch(/background:\s*var\(--fill-active\)/);
    const bot = ruleBody('.cockpit .ck-potato-bubble--bot');
    expect(bot).toMatch(/background:\s*transparent/);
    expect(bot).toMatch(/border:\s*1px solid var\(--sep\)/);
  });

  it('sets bubble text at 13 px and the list at 240 px', () => {
    expect(ruleBody('.cockpit .ck-potato-bubble')).toMatch(/font-size:\s*13px/);
    expect(ruleBody('.cockpit .ck-potato-list')).toMatch(/width:\s*240px/);
  });
});
