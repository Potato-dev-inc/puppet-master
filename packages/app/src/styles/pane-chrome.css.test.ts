// @vitest-environment node
// A question card in the Terminal tab must not take space from the terminal: any change to the terminal's
// height resizes the PTY, and an inline TUI reprints its whole screen on every resize. The dock floats.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./pane-chrome.css', import.meta.url), 'utf8');

/** The declarations of the first rule that starts with exactly `selector {`. */
function rule(selector: string): string {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no rule for ${selector}`);
  return css.slice(css.indexOf('{', start) + 1, css.indexOf('}', start));
}

describe('the pane dock does not resize the terminal', () => {
  it('floats over the terminal (out of the flow) and is anchored in the terminal wrapper', () => {
    expect(rule('.cockpit .ck-dock')).toMatch(/position:\s*absolute/);
    expect(rule('.cockpit .ck-term-wrap')).toMatch(/position:\s*relative/);
  });

  it('is opaque, so the card stays readable over the terminal', () => {
    expect(rule('.cockpit .ck-dock')).toMatch(/background:\s*var\(--bg\)/);
  });
});
