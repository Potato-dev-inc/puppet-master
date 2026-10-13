import { describe, expect, it } from 'vitest';
import {
  anchorFindings,
  formatFollowup,
  matchFile,
  newLineNumbers,
  normalizePath,
  severityGlyph,
  severityLabel,
  type ReviewFinding,
} from './review-diff';

const BODY = [
  '@@ -1,4 +1,4 @@',
  ' alpha', // 1
  '-bravo', // deleted
  '+bravo two', // 2
  ' charlie', // 3
  '+delta', // 4
  '@@ -10,2 +20,3 @@',
  ' echo', // 20
  '+foxtrot', // 21
  '\\ No newline at end of file',
  ' golf', // 22
];

describe('newLineNumbers', () => {
  it('numbers context and added lines in the new file across two hunks', () => {
    expect(newLineNumbers(BODY)).toEqual([null, 1, null, 2, 3, 4, null, 20, 21, null, 22]);
  });

  it('returns null for everything before the first hunk header', () => {
    expect(newLineNumbers(['Binary files a and b differ'])).toEqual([null]);
  });
});

describe('normalizePath', () => {
  it('turns backslashes into slashes, strips leading ./ and trims', () => {
    expect(normalizePath('  .\\src\\a.ts ')).toBe('src/a.ts');
    expect(normalizePath('./b/c.ts')).toBe('b/c.ts');
    expect(normalizePath('plain.txt')).toBe('plain.txt');
  });
});

describe('matchFile', () => {
  const files = ['packages/app/src/a.ts', 'packages/app/src/b.ts', 'other/a.ts'];

  it('matches an exact path after normalising', () => {
    expect(matchFile('.\\packages\\app\\src\\a.ts', files)).toBe('packages/app/src/a.ts');
  });

  it('matches a unique suffix in either direction', () => {
    expect(matchFile('src/b.ts', files)).toBe('packages/app/src/b.ts');
    expect(matchFile('/x/packages/app/src/b.ts', files)).toBe('packages/app/src/b.ts');
  });

  it('returns null when the suffix is ambiguous', () => {
    expect(matchFile('a.ts', files)).toBeNull();
  });

  it('returns null when nothing matches', () => {
    expect(matchFile('missing.ts', files)).toBeNull();
    expect(matchFile('', files)).toBeNull();
  });
});

describe('anchorFindings', () => {
  const bodies = new Map<string, string[]>([
    ['src/a.ts', BODY],
    ['src/b.ts', ['@@ -1 +1 @@', '-x', '+y']],
  ]);
  const f = (over: Partial<ReviewFinding>): ReviewFinding => ({
    id: 'x',
    file: 'src/a.ts',
    line: 2,
    severity: 'medium',
    text: 'note',
    ...over,
  });

  it('anchors findings on a new-file line of a matched file, and file-level findings too', () => {
    const a = f({ id: 'a', line: 2 });
    const header = f({ id: 'h', line: null });
    const { byFile, unanchored } = anchorFindings([a, header], bodies);
    expect(byFile.get('src/a.ts')).toEqual([a, header]);
    expect(unanchored).toEqual([]);
  });

  it('leaves findings unanchored when the file is not in the diff', () => {
    const missing = f({ id: 'm', file: 'src/nope.ts' });
    const { byFile, unanchored } = anchorFindings([missing], bodies);
    expect(byFile.size).toBe(0);
    expect(unanchored).toEqual([missing]);
  });

  it('leaves findings unanchored when the line is not a new-file line in any hunk', () => {
    // Line 5 falls between the hunks of src/a.ts, so it is not in the diff.
    const outside = f({ id: 'o', line: 5 });
    const { byFile, unanchored } = anchorFindings([outside], bodies);
    expect(unanchored).toEqual([outside]);
    expect(byFile.size).toBe(0);
  });

  it('anchors a finding on an added line of a short file, matched by suffix', () => {
    const added = f({ id: 'k', file: 'b.ts', line: 1 });
    expect(anchorFindings([added], bodies).byFile.get('src/b.ts')).toEqual([added]);
  });
});

describe('labels and follow-up text', () => {
  it('maps severities to plain glyphs and labels', () => {
    expect([severityGlyph('high'), severityGlyph('medium'), severityGlyph('low'), severityGlyph('info')]).toEqual([
      '!!',
      '!',
      '·',
      '–',
    ]);
    expect([severityLabel('high'), severityLabel('medium'), severityLabel('low'), severityLabel('info')]).toEqual([
      'High',
      'Medium',
      'Low',
      'Note',
    ]);
  });

  it('formats a follow-up with and without a line', () => {
    expect(formatFollowup('src/a.ts', 12, 'Null check missing')).toBe('src/a.ts:12 — Null check missing');
    expect(formatFollowup('src/a.ts', null, 'Split this file')).toBe('src/a.ts — Split this file');
  });
});
