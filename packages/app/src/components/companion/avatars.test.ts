import { describe, expect, it } from 'vitest';
import type { AvatarId } from '../../lib/companion';
import { AVATARS, GRID, parseBitmap } from './avatars';

const IDS: AvatarId[] = ['mochi', 'bit', 'nib', 'tofu', 'lumen', 'hoot', 'sprout', 'kit'];
const NOT_SYMMETRIC: AvatarId[] = ['tofu', 'sprout'];

describe('avatar bitmaps', () => {
  it('has exactly the eight faces from the spec', () => {
    expect(Object.keys(AVATARS).sort()).toEqual([...IDS].sort());
    expect(IDS.map((id) => AVATARS[id].name)).toEqual(['Mochi', 'Bit', 'Nib', 'Tofu', 'Lumen', 'Hoot', 'Sprout', 'Kit']);
  });

  it.each(IDS)('%s is a 12 by 12 grid', (id) => {
    const rows = AVATARS[id].rows;
    expect(rows).toHaveLength(GRID);
    for (const row of rows) expect(row).toHaveLength(GRID);
    expect(() => parseBitmap(rows)).not.toThrow();
  });

  it.each(IDS)('%s has eyes', (id) => {
    const eyes = AVATARS[id].rows.join('').split('e').length - 1;
    expect(eyes).toBeGreaterThan(0);
  });

  it.each(IDS.filter((id) => !NOT_SYMMETRIC.includes(id)))('%s is mirror-symmetric row by row', (id) => {
    for (const row of AVATARS[id].rows) {
      expect(row).toBe([...row].reverse().join(''));
    }
  });

  it('uses the dot eye style for Bit and Hoot, and the hole style for the rest', () => {
    expect(AVATARS.bit.eyes).toBe('dot');
    expect(AVATARS.hoot.eyes).toBe('dot');
    for (const id of IDS.filter((x) => x !== 'bit' && x !== 'hoot')) expect(AVATARS[id].eyes).toBe('hole');
  });

  it('only uses monochrome cell characters', () => {
    for (const id of IDS) {
      expect(AVATARS[id].rows.join('')).toMatch(/^[#+.e]+$/);
    }
  });
});

describe('parseBitmap', () => {
  const blank = Array.from({ length: GRID }, () => '.'.repeat(GRID));

  it('reads each character as its cell kind', () => {
    const rows = [...blank];
    rows[0] = '#+e.#+e.#+e.';
    const grid = parseBitmap(rows);
    expect(grid[0]!.slice(0, 3)).toEqual(['full', 'dim', 'eye']);
    expect(grid[0]!.slice(3, 6)).toEqual(['empty', 'full', 'dim']);
    expect(grid[1]!.every((cell) => cell === 'empty')).toBe(true);
  });

  it('refuses a bitmap that is not 12 rows', () => {
    expect(() => parseBitmap(blank.slice(1))).toThrow('needs 12 rows');
  });

  it('refuses a row that is not 12 cells', () => {
    const rows = [...blank];
    rows[3] = '.'.repeat(11);
    expect(() => parseBitmap(rows)).toThrow('row 3 needs 12 cells');
  });

  it('refuses an unknown cell character', () => {
    const rows = [...blank];
    rows[5] = 'x'.repeat(GRID);
    expect(() => parseBitmap(rows)).toThrow('unknown cell "x"');
  });
});
