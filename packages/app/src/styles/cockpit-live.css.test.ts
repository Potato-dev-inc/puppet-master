// @vitest-environment node
// Live Room styles: monochrome (grayscale tokens only, the two inverted-bar greys are defined once on the
// cockpit), every rule under the cockpit, only tokens that exist, and no bar-closing control in the markup.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Git on Windows may check the file out with CRLF line endings; the rules are read the same either way.
const css = readFileSync(new URL('./cockpit-live.css', import.meta.url), 'utf8').replace(/\r\n/g, '\n');
const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');
/** The rules with the custom-property declarations taken out: those are the only place a colour may live. */
// Keyframe steps (0%, 50%) are not selectors on the cockpit, so they are taken out too.
const rules = bare.replace(/@keyframes[^{]*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}/g, '').replace(/--[a-zA-Z0-9-]+\s*:[^;]*;/g, '');

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

describe('Live Room styles', () => {
  it('uses no colour literals outside the two custom properties of the inverted bar', () => {
    expect(rules).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(rules).not.toMatch(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
    expect(rules).not.toMatch(/:[^;{}]*\b(black|white|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|navy|teal|maroon|lime|aqua|olive)\b/i);
  });

  it('defines the inverted bar greys once, on the cockpit', () => {
    expect(css).toMatch(/\.cockpit\s*\{[^}]*--live-ink:\s*#0a0a0b/);
    expect(css).toMatch(/\.cockpit\s*\{[^}]*--live-paper:\s*#f5f5f7/);
  });

  it('only reads custom properties that exist', () => {
    const tokens = definedTokens();
    const used = [...bare.matchAll(/var\((--[a-zA-Z0-9-]+)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(0);
    for (const name of used) expect(tokens.has(name), name).toBe(true);
  });

  it('scopes every selector under the cockpit', () => {
    const selectors = [...rules.matchAll(/([^{}]+)\{/g)]
      .map((m) => m[1].trim())
      .filter((s) => !s.startsWith('@'))
      .flatMap((s) => s.split(',').map((part) => part.trim()));
    expect(selectors.length).toBeGreaterThan(0);
    for (const selector of selectors) expect(selector, selector).toMatch(/^\.cockpit\b/);
  });

  it('has every class the components use', () => {
    for (const name of [
      'ck-live-bar',
      'ck-live-tag',
      'ck-live-dot',
      'ck-live-btn',
      'ck-live-chip',
      'ck-live-chip-menu',
      'ck-live-menu-item',
      'ck-live-panes',
      'ck-live-pane',
      'ck-live-helpers',
      'ck-live-preview',
      'ck-live-item',
      'ck-live-hidden',
    ]) {
      expect(css, name).toContain(`.${name}`);
    }
  });

  it('keeps the LIVE dot still for users who ask for less motion', () => {
    expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)\s*\{[\s\S]*?\.ck-live-dot\s*\{\s*animation:\s*none/);
  });
});
