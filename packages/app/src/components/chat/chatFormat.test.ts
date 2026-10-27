import { describe, expect, it } from 'vitest';
import { baseName, formatDuration, plural } from './chatFormat';

describe('chat text helpers', () => {
  it('formats durations in seconds and minutes', () => {
    expect(formatDuration(400)).toBe('<1s');
    expect(formatDuration(48_000)).toBe('48s');
    expect(formatDuration(125_000)).toBe('2m 5s');
  });

  it('pluralises a count', () => {
    expect(plural(1, 'file')).toBe('1 file');
    expect(plural(0, 'file')).toBe('0 files');
    expect(plural(6, 'time')).toBe('6 times');
  });

  it('keeps the last part of a path for both slashes', () => {
    expect(baseName('src/lib/a.ts')).toBe('a.ts');
    expect(baseName('C:\\work\\b.rs')).toBe('b.rs');
    expect(baseName('folder/')).toBe('folder');
    expect(baseName('')).toBe('');
  });
});
