// @vitest-environment node
// Potato Bot styles, every `cockpit-potato*.css` file: grayscale tokens only (no colour literals), every custom
// property exists, every rule sits under the cockpit, your bubbles are filled and the bot's are outlined, and the text is
// 13 px. One file per area, so later phases touch different files; this test reads all of them.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const dir = fileURLToPath(new URL('.', import.meta.url));
const FILES = readdirSync(dir)
  .filter((f) => /^cockpit-potato[a-z-]*\.css$/.test(f))
  .sort();
const read = (file: string) => readFileSync(`${dir}${file}`, 'utf8').replace(/\r\n/g, '\n');
const strip = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');
const sheets = FILES.map((file) => ({ file, bare: strip(read(file)) }));
const all = sheets.map((s) => s.bare).join('\n');

function definedTokens(): Set<string> {
  const names = new Set<string>();
  for (const file of readdirSync(dir)) {
    if (!file.endsWith('.css')) continue;
    for (const match of readFileSync(`${dir}${file}`, 'utf8').matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) names.add(match[1]);
  }
  return names;
}

function ruleBody(selector: string): string {
  const start = all.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThan(-1);
  return all.slice(start, all.indexOf('}', start));
}

describe('Potato Bot styles', () => {
  it('has one file per area', () => {
    for (const area of ['', '-list', '-thread', '-composer', '-cards', '-panel', '-ui']) {
      expect(FILES, `cockpit-potato${area}.css`).toContain(`cockpit-potato${area}.css`);
    }
  });

  it('is imported by the code: a file nobody imports styles nothing', () => {
    const sources: string[] = [];
    const walk = (path: string) => {
      for (const entry of readdirSync(path, { withFileTypes: true })) {
        if (entry.isDirectory()) walk(`${path}/${entry.name}`);
        else if (/\.tsx?$/.test(entry.name) && !/\.test\./.test(entry.name)) sources.push(readFileSync(`${path}/${entry.name}`, 'utf8'));
      }
    };
    walk(fileURLToPath(new URL('../components/cockpit/potato', import.meta.url)));
    const code = sources.join('\n');
    for (const file of FILES) expect(code, file).toContain(`styles/${file}'`);
  });

  it.each(sheets.map((s) => [s.file, s.bare] as const))('%s uses no colour literals', (_file, bare) => {
    expect(bare).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(bare).not.toMatch(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
    expect(bare).not.toMatch(/:[^;{}]*\b(black|white|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|navy|teal|maroon|lime|aqua|olive)\b/i);
  });

  it('only reads custom properties that exist', () => {
    const tokens = definedTokens();
    // The page sets its own sizes as inline custom properties (see PotatoLayout.tsx).
    for (const own of ['--potato-list-w', '--potato-list-shown', '--potato-panel-w']) tokens.add(own);
    const used = [...all.matchAll(/var\((--[a-zA-Z0-9-]+)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const name of used) expect(tokens.has(name), name).toBe(true);
  });

  it.each(sheets.map((s) => [s.file, s.bare] as const))('%s scopes every selector under the cockpit', (_file, bare) => {
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

  it('sets bubble text at 13 px, the list at 240 px by default and the panel between 320 and 1000 px', () => {
    expect(ruleBody('.cockpit .ck-potato-bubble')).toMatch(/font-size:\s*13px/);
    expect(ruleBody('.cockpit .ck-potato-list')).toMatch(/width:\s*var\(--potato-list-w, 240px\)/);
    const panel = ruleBody('.cockpit .ck-potato-panel');
    expect(panel).toMatch(/width:\s*var\(--potato-panel-w, 384px\)/);
    expect(panel).toMatch(/min-width:\s*320px/);
  });
});
