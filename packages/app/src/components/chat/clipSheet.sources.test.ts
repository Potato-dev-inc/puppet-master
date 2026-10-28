// The clip files hold nothing that can send data off the PC, and the phone never loads the sheet. The files are read
// as text: comments are cut first so a sentence that explains a rule does not trip the rule.
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = path.resolve(__dirname, '../..');

const CLIP_FILES = [
  'components/chat/ClipSheet.tsx',
  'components/chat/useClipMaker.ts',
  'lib/clip.ts',
  'lib/clipDraw.ts',
  'lib/clipEncode.ts',
  'lib/clipGuard.ts',
  'lib/clipTimeline.ts',
  'lib/clipTypes.ts',
  'lib/mp4Mux.ts',
];

function code(file: string): string {
  return fs
    .readFileSync(path.join(SRC, file), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/(^|[^:'"`])\/\/.*$/gm, '$1');
}

// Built from pieces so this file does not hold the words it forbids.
const FORBIDDEN = [
  ['fe', 'tch('],
  ['navigator.', 'share'],
  ['navigator.', 'sendBeacon'],
  ['XMLHttp', 'Request'],
  ['Web', 'Socket'],
  ['Event', 'Source'],
  ['window.', 'open('],
].map((parts) => parts.join(''));

describe('clip files have no way to send data out', () => {
  for (const file of CLIP_FILES) {
    it(`${file} has no network or share call`, () => {
      const text = code(file);
      for (const word of FORBIDDEN) expect(text, `${file} contains ${word}`).not.toContain(word);
    });
  }

  it('the clip files do not write into the page as markup', () => {
    for (const file of CLIP_FILES) {
      const text = code(file);
      expect(text).not.toContain('innerHTML');
      expect(text).not.toContain('dangerouslySetInnerHTML');
    }
  });

  it('the clip files do not write to a pane', () => {
    for (const file of CLIP_FILES) {
      expect(code(file)).not.toMatch(/write_input|press_key|answer_prompt/);
    }
  });
});

/** The relative files a source file imports (static and dynamic), resolved to real files. */
function importsOf(file: string): string[] {
  const text = fs.readFileSync(file, 'utf8');
  const found: string[] = [];
  for (const m of text.matchAll(/(?:from\s+|import\s*\(\s*|import\s+)['"](\.[^'"]+)['"]/g)) {
    const base = path.resolve(path.dirname(file), m[1]);
    for (const candidate of [base, `${base}.ts`, `${base}.tsx`, path.join(base, 'index.ts'), path.join(base, 'index.tsx')]) {
      if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) {
        found.push(candidate);
        break;
      }
    }
  }
  return found;
}

describe('the phone bundle', () => {
  it('never reaches the clip sheet, the clip maker or the clip code', () => {
    const seen = new Set<string>();
    const queue = [path.join(SRC, 'PwaApp.tsx')];
    while (queue.length > 0) {
      const next = queue.pop() as string;
      if (seen.has(next)) continue;
      seen.add(next);
      if (/\.(css)$/.test(next)) continue;
      queue.push(...importsOf(next));
    }
    const names = [...seen].map((f) => path.relative(SRC, f).replace(/\\/g, '/'));
    expect(names.length).toBeGreaterThan(20);
    expect(names.filter((n) => /clip|mp4Mux/i.test(n))).toEqual([]);
  });

  it('PwaApp does not mention the clip at all', () => {
    expect(fs.readFileSync(path.join(SRC, 'PwaApp.tsx'), 'utf8').toLowerCase()).not.toContain('clip');
  });
});
