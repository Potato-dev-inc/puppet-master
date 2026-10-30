// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SPLIT_MEMORY_KEY, clearSplitMemory, parseSplitMemory, readSplitMemory, restorableSplit, writeSplitMemory } from './split-memory';

const saved = { left: 'p1', right: 'p2', ratio: 0.4 };
const both = (id: string) => id === 'p1' || id === 'p2';

beforeEach(() => localStorage.clear());
afterEach(() => vi.restoreAllMocks());

describe('split memory: storage', () => {
  it('writes, reads back, and clears the saved split', () => {
    writeSplitMemory(saved);
    expect(readSplitMemory()).toEqual(saved);
    clearSplitMemory();
    expect(readSplitMemory()).toBeNull();
  });

  it('keeps the split under its own key', () => {
    writeSplitMemory(saved);
    expect(JSON.parse(localStorage.getItem(SPLIT_MEMORY_KEY) ?? 'null')).toEqual(saved);
  });

  it('a blocked or full storage is not an error: the split just is not remembered', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(() => writeSplitMemory(saved)).not.toThrow();
    expect(() => clearSplitMemory()).not.toThrow();
    expect(readSplitMemory()).toBeNull();
  });
});

describe('split memory: parsing', () => {
  it('reads only the expected shape; anything else is no split', () => {
    expect(parseSplitMemory(null)).toBeNull();
    expect(parseSplitMemory('not json')).toBeNull();
    expect(parseSplitMemory('null')).toBeNull();
    expect(parseSplitMemory(JSON.stringify({ left: 'p1' }))).toBeNull();
    expect(parseSplitMemory(JSON.stringify({ left: 'p1', right: '' }))).toBeNull();
  });

  it('a ratio that is not inside (0, 1) falls back to the even split', () => {
    expect(parseSplitMemory(JSON.stringify({ left: 'p1', right: 'p2', ratio: 'wide' }))?.ratio).toBe(0.5);
    expect(parseSplitMemory(JSON.stringify({ left: 'p1', right: 'p2', ratio: 0 }))?.ratio).toBe(0.5);
    expect(parseSplitMemory(JSON.stringify({ left: 'p1', right: 'p2', ratio: 1.5 }))?.ratio).toBe(0.5);
  });
});

describe('split memory: restore on start', () => {
  it('comes back when both panes still exist and the window is wide enough', () => {
    expect(restorableSplit(saved, { width: 1200, hasPane: both })).toEqual(saved);
  });

  it('a missing pane means no split (the saved text is kept)', () => {
    writeSplitMemory(saved);
    expect(restorableSplit(saved, { width: 1200, hasPane: (id) => id === 'p1' })).toBeNull();
    expect(restorableSplit(saved, { width: 1200, hasPane: (id) => id === 'p2' })).toBeNull();
    expect(readSplitMemory()).toEqual(saved);
  });

  it('no saved split, the same pane twice, or a narrow window: no split', () => {
    expect(restorableSplit(null, { width: 1200, hasPane: both })).toBeNull();
    expect(restorableSplit({ ...saved, right: 'p1' }, { width: 1200, hasPane: both })).toBeNull();
    expect(restorableSplit(saved, { width: 800, hasPane: both })).toBeNull();
  });
});
