import { describe, expect, it } from 'vitest';
import { baseName, fileFolder, filesCount, formatTokens, readContext, shortFolder, writtenFiles } from './railFormat';

const usage = (inputTokens: number, outputTokens: number, contextWindow: number | null) => ({
  kind: 'usage' as const,
  id: 'u',
  at: null,
  inputTokens,
  outputTokens,
  contextWindow,
});

describe('formatTokens', () => {
  it('keeps small numbers and shortens big ones', () => {
    expect(formatTokens(850)).toBe('850');
    expect(formatTokens(1200)).toBe('1.2k');
    expect(formatTokens(48_000)).toBe('48k');
    expect(formatTokens(200_000)).toBe('200k');
    expect(formatTokens(1_000_000)).toBe('1M');
  });
});

describe('readContext', () => {
  it('counts input and output against the reported window', () => {
    expect(readContext(usage(47_000, 1000, 200_000))).toMatchObject({ used: 48_000, window: 200_000, percent: 24, input: 47_000, output: 1000 });
  });
  it('is null when nothing was reported, never a guess', () => {
    expect(readContext(null)).toBeNull();
    expect(readContext(usage(5000, 100, null))).toBeNull();
    expect(readContext(usage(5000, 100, 0))).toBeNull();
  });
  it('never goes over 100', () => {
    expect(readContext(usage(300_000, 0, 200_000))?.percent).toBe(100);
  });
});

describe('paths', () => {
  it('splits names and folders on either slash', () => {
    expect(baseName('C:\\work\\shop\\src\\app.ts')).toBe('app.ts');
    expect(shortFolder('C:\\work\\shop\\app')).toBe('shop/app');
  });
  it('shows a file folder relative to the pane folder', () => {
    expect(fileFolder('C:\\work\\shop\\src\\app.ts', 'C:\\work\\shop')).toBe('src');
    expect(fileFolder('C:\\work\\shop\\notes.txt', 'C:\\work\\shop')).toBe('');
    expect(fileFolder('D:\\other\\place\\x.md', 'C:\\work\\shop')).toBe('other/place');
  });
  it('keeps only written files and counts them', () => {
    const files = [
      { path: 'a', tool: 'Read', wrote: false },
      { path: 'b', tool: 'Edit', wrote: true },
    ];
    expect(writtenFiles(files).map((f) => f.path)).toEqual(['b']);
    expect(filesCount(1)).toBe('+1 file');
    expect(filesCount(3)).toBe('+3 files');
  });
});
