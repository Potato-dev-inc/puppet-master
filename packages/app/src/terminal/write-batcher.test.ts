import { describe, expect, it } from 'vitest';
import { mergeChunks } from './write-batcher';

describe('mergeChunks', () => {
  it('returns an empty array for no chunks', () => {
    expect(mergeChunks([])).toEqual(new Uint8Array([]));
  });

  it('merges multiple chunks in order', () => {
    const merged = mergeChunks([
      new Uint8Array([1, 2]),
      new Uint8Array([3]),
      new Uint8Array([4, 5]),
    ]);
    expect(Array.from(merged)).toEqual([1, 2, 3, 4, 5]);
  });
});
