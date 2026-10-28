import { describe, expect, it } from 'vitest';
import { kindText, snippetParts, whenText } from './chatSearch';

describe('snippetParts', () => {
  it('cuts the snippet at the marked ranges', () => {
    expect(snippetParts('move the deploy script', [[9, 15]])).toEqual([
      { text: 'move the ', hit: false },
      { text: 'deploy', hit: true },
      { text: ' script', hit: false },
    ]);
  });

  it('keeps a snippet with no marks whole, and ignores ranges that make no sense', () => {
    expect(snippetParts('plain', [])).toEqual([{ text: 'plain', hit: false }]);
    // Past the end, empty, and overlapping the one before: all skipped.
    expect(snippetParts('plain', [[4, 9], [3, 3], [2, 4], [1, 2]])).toEqual([
      { text: 'pl', hit: false },
      { text: 'ai', hit: true },
      { text: 'n', hit: false },
    ]);
  });
});

describe('whenText', () => {
  const now = new Date(2026, 9, 28, 15, 30).getTime();
  const at = (y: number, m: number, d: number, h = 12, min = 0) => new Date(y, m, d, h, min).getTime();

  it('says minutes, hours, yesterday, then the date', () => {
    expect(whenText(now - 20_000, now)).toBe('Just now');
    expect(whenText(now - 5 * 60_000, now)).toBe('5 min ago');
    expect(whenText(at(2026, 9, 28, 11, 30), now)).toBe('4 h ago');
    expect(whenText(at(2026, 9, 27, 23, 0), now)).toBe('Yesterday');
    expect(whenText(at(2026, 9, 12), now)).toBe('12 Oct');
    expect(whenText(at(2025, 11, 3), now)).toBe('3 Dec 2025');
  });
});

describe('kindText', () => {
  it('names who spoke', () => {
    expect([kindText('user'), kindText('assistant'), kindText('tool')]).toEqual(['You', 'Agent', 'Tool']);
  });
});
