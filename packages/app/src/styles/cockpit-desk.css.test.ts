// @vitest-environment node
// My computers styles: cockpit tokens only (no colour literals), every var() used is defined by the app's styles,
// and the sheet is imported by the page, so it is not dead CSS.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const dir = fileURLToPath(new URL('.', import.meta.url));
const css = readFileSync(`${dir}cockpit-desk.css`, 'utf8').replace(/\r\n/g, '\n');
const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');

function definedTokens(): Set<string> {
  const names = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.css')) continue;
    for (const match of readFileSync(`${dir}${file}`, 'utf8').matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) names.add(match[1]);
  }
  return names;
}

describe('My computers styles', () => {
  it('uses no colour literals', () => {
    expect(bare).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(bare).not.toMatch(/\b(rgb|rgba|hsl|hsla)\(/i);
  });

  it('uses only tokens the app defines', () => {
    const defined = definedTokens();
    const used = [...bare.matchAll(/var\((--[a-zA-Z0-9-]+)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const token of new Set(used)) expect(defined.has(token), token).toBe(true);
  });

  it('every rule sits under the cockpit, except keyframes', () => {
    const selectors = [...bare.matchAll(/([^{}]+)\{/g)].map((m) => m[1].trim()).filter((s) => !s.startsWith('@') && !/^(from|to|\d+%)$/.test(s));
    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) expect(selector.startsWith('.cockpit '), selector).toBe(true);
  });

  it('is imported by the page', () => {
    const page = readFileSync(`${dir}../components/desk/DeskPage.tsx`, 'utf8');
    expect(page).toContain("'../../styles/cockpit-desk.css'");
  });
});
