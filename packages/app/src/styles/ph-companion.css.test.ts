// @vitest-environment node
// The phone chat view is `position: absolute; inset: 0` (phone-chat.css). Inside the companion home it must be
// anchored to its own area, or it covers the face, the feed and the footer (found in the live check).
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./ph-companion.css', import.meta.url), 'utf8');

function rule(selector: string): string {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no rule for ${selector}`);
  return css.slice(css.indexOf('{', start) + 1, css.indexOf('}', start));
}

describe('the companion home keeps its chat in its own area', () => {
  it('anchors the absolutely placed chat view', () => {
    expect(rule('.ph-comp-chat')).toMatch(/position:\s*relative/);
  });
});
