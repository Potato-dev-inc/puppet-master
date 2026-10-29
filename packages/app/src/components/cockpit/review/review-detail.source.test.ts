// @vitest-environment node
// Source guards for the Review Desk detail. Titles, file names and diff lines come from the agent, so they are
// React text nodes only (no markup, no markdown). Generic code never names an agent.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const FILES = ['ReviewDetail.tsx', 'KeepSheet.tsx', 'FixSheet.tsx'];

const sources = FILES.map((name) => ({
  name,
  text: readFileSync(new URL(`./${name}`, import.meta.url), 'utf8').replace(/\r\n/g, '\n'),
}));

describe('review detail sources', () => {
  it('never renders agent text as markup', () => {
    for (const { name, text } of sources) {
      expect(text, name).not.toMatch(/dangerouslySetInnerHTML/);
      expect(text, name).not.toMatch(/innerHTML/);
      expect(text, name).not.toMatch(/\bmarked\b/);
      expect(text, name).not.toMatch(/markdown/i);
    }
  });

  it('names no agent (generic code)', () => {
    for (const { name, text } of sources) {
      expect(text, name).not.toMatch(/claude|codex|cursor|opencode|\bagy\b|kilo/i);
    }
  });

  it('only fixes the draft through the fix call, never sends text to a pane', () => {
    for (const { name, text } of sources) {
      expect(text, name).not.toMatch(/\bwrite_input\b|\bsend_keys\b|\bpress_key\b|TypingPermit|\boutbox\b/);
    }
  });
});
