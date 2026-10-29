// @vitest-environment node
// The desk folder is plain React text. No markup is injected (agent text and diff lines are titles and text nodes
// only), and the generic desk code never names an agent.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const dir = fileURLToPath(new URL('.', import.meta.url));
// Git on Windows may check the files out with CRLF line endings; the rules read the same either way.
const sources = readdirSync(dir)
  .filter((file) => /\.tsx?$/.test(file) && !/\.test\.tsx?$/.test(file))
  .map((file) => ({ file, text: readFileSync(`${dir}${file}`, 'utf8').replace(/\r\n/g, '\n') }));

describe('Review Desk source', () => {
  it('has the desk, the row, the detail, its two sheets, the keys and the count hook', () => {
    expect(sources.map((s) => s.file).sort()).toEqual([
      'FixSheet.tsx',
      'KeepSheet.tsx',
      'ReviewDesk.tsx',
      'ReviewDetail.tsx',
      'ReviewRow.tsx',
      'review-keys.ts',
      'useReviewCount.ts',
    ]);
  });

  it('injects no markup: no dangerouslySetInnerHTML, innerHTML, marked or a Markdown renderer', () => {
    for (const { file, text } of sources) {
      expect(text, file).not.toMatch(/dangerouslySetInnerHTML/);
      expect(text, file).not.toMatch(/innerHTML/);
      expect(text, file).not.toMatch(/from ['"]marked['"]/);
      expect(text, file).not.toMatch(/Markdown/);
    }
  });

  it('names no agent in generic desk code', () => {
    for (const { file, text } of sources) {
      expect(text, file).not.toMatch(/\b(claude|codex|cursor|opencode|agy|kilo)\b/i);
    }
  });
});
