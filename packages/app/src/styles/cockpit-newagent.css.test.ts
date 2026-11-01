// @vitest-environment node
// The New agent page is quiet and charcoal: colours come from the cockpit tokens only (so it follows light and dark
// and the theme another change sets), and every rule sits under the cockpit so it cannot leak onto other pages.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./cockpit-newagent.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');

/** Every custom property defined anywhere in the styles folder. */
function definedTokens(): Set<string> {
  const dir = fileURLToPath(new URL('.', import.meta.url));
  const names = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.css')) continue;
    for (const match of readFileSync(`${dir}${file}`, 'utf8').matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) names.add(match[1]);
  }
  return names;
}

describe('new agent page styles', () => {
  it('uses no colour literals (hex, colour functions or named colours)', () => {
    expect(bare).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(bare).not.toMatch(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
    expect(bare).not.toMatch(/:[^;{}]*\b(black|white|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|navy|teal|maroon|lime|aqua|olive)\b/i);
  });

  it('only reads custom properties that exist (--accent is optional: the Send button falls back to the primary colour)', () => {
    const tokens = definedTokens();
    const used = [...bare.matchAll(/var\((--[a-zA-Z0-9-]+)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const name of used) expect(tokens.has(name) || name === '--accent', name).toBe(true);
  });

  it('uses --accent only for the Send button', () => {
    const rules = [...bare.matchAll(/([^{}]+)\{([^}]*)\}/g)];
    const using = rules.filter(([, , body]) => body.includes('--accent')).map(([, selector]) => selector.trim());
    expect(using).toEqual(['.cockpit .ck-na-send']);
    expect(bare).toMatch(/var\(--accent,\s*var\(--primary-bg\)\)/);
  });

  it('scopes every selector under the cockpit', () => {
    const selectors = [...bare.matchAll(/([^{}]+)\{/g)]
      .map((m) => m[1].trim())
      .filter((s) => !s.startsWith('@'))
      .flatMap((s) => s.split(',').map((part) => part.trim()));
    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit \.ck-(na|side-new)/);
  });

  it('has the classes the page uses', () => {
    for (const name of ['ck-na', 'ck-na-top', 'ck-na-center', 'ck-na-greet', 'ck-na-composer', 'ck-na-input', 'ck-na-chip', 'ck-na-menu', 'ck-na-send', 'ck-na-hc', 'ck-na-addrow', 'ck-na-link']) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('keeps the Add card row dashed', () => {
    const start = css.indexOf('.cockpit .ck-na-addrow {');
    expect(start).toBeGreaterThan(-1);
    expect(css.slice(start, css.indexOf('}', start))).toMatch(/border:\s*1px dashed/);
  });
});
