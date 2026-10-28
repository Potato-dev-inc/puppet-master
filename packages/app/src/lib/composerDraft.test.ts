// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearDraft,
  DRAFT_MAX_AGE_MS,
  DRAFT_MAX_CHARS,
  DRAFT_MAX_COUNT,
  keepFresh,
  LS_COMPOSER_DRAFTS,
  readDraft,
  writeDraft,
} from './composerDraft';

const NOW = Date.UTC(2026, 9, 28, 12);

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('composer drafts', () => {
  it('keeps a draft per pane and reads it back', () => {
    writeDraft('pane-a', 'half a sentence', NOW);
    writeDraft('pane-b', 'another one', NOW);
    expect(readDraft('pane-a', NOW)).toBe('half a sentence');
    expect(readDraft('pane-b', NOW)).toBe('another one');
    expect(readDraft('pane-c', NOW)).toBe('');
  });

  it('clearDraft forgets only that pane', () => {
    writeDraft('pane-a', 'keep me', NOW);
    writeDraft('pane-b', 'send me', NOW);
    clearDraft('pane-b');
    expect(readDraft('pane-b', NOW)).toBe('');
    expect(readDraft('pane-a', NOW)).toBe('keep me');
  });

  it('a blank text removes the draft instead of keeping spaces', () => {
    writeDraft('pane-a', 'text', NOW);
    writeDraft('pane-a', '   \n ', NOW);
    expect(readDraft('pane-a', NOW)).toBe('');
  });

  it('drops a draft not written for 7 days, and keeps one written 6 days ago', () => {
    writeDraft('old', 'old text', NOW - DRAFT_MAX_AGE_MS - 1);
    writeDraft('recent', 'recent text', NOW - (DRAFT_MAX_AGE_MS - 1000));
    expect(readDraft('old', NOW)).toBe('');
    expect(readDraft('recent', NOW)).toBe('recent text');
    // The next write removes the stale entry from storage too, not only hides it.
    writeDraft('third', 'x', NOW);
    expect(JSON.parse(localStorage.getItem(LS_COMPOSER_DRAFTS) ?? '{}')).not.toHaveProperty('old');
  });

  it('keeps at most 50 drafts, the newest', () => {
    for (let i = 0; i < DRAFT_MAX_COUNT + 5; i += 1) writeDraft(`pane-${i}`, `text ${i}`, NOW + i);
    const stored = JSON.parse(localStorage.getItem(LS_COMPOSER_DRAFTS) ?? '{}') as Record<string, unknown>;
    expect(Object.keys(stored)).toHaveLength(DRAFT_MAX_COUNT);
    expect(readDraft('pane-0', NOW + 100)).toBe('');
    expect(readDraft(`pane-${DRAFT_MAX_COUNT + 4}`, NOW + 100)).toBe(`text ${DRAFT_MAX_COUNT + 4}`);
  });

  it('a text over 20000 characters is not kept, and an older kept one is removed', () => {
    writeDraft('pane-a', 'short', NOW);
    writeDraft('pane-a', 'x'.repeat(DRAFT_MAX_CHARS + 1), NOW);
    expect(readDraft('pane-a', NOW)).toBe('');
    writeDraft('pane-a', 'x'.repeat(DRAFT_MAX_CHARS), NOW);
    expect(readDraft('pane-a', NOW)).toHaveLength(DRAFT_MAX_CHARS);
  });

  it('a broken stored value reads as no drafts, and a write starts over', () => {
    localStorage.setItem(LS_COMPOSER_DRAFTS, '{not json');
    expect(readDraft('pane-a', NOW)).toBe('');
    writeDraft('pane-a', 'fresh', NOW);
    expect(readDraft('pane-a', NOW)).toBe('fresh');
  });

  it('ignores entries of the wrong shape', () => {
    localStorage.setItem(LS_COMPOSER_DRAFTS, JSON.stringify({ bad: 'text', worse: { at: 'x' } }));
    expect(readDraft('bad', NOW)).toBe('');
  });

  it('storage that throws never breaks the caller', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    expect(readDraft('pane-a', NOW)).toBe('');
    expect(() => writeDraft('pane-a', 'text', NOW)).not.toThrow();
    expect(() => clearDraft('pane-a')).not.toThrow();
  });

  it('keepFresh is pure: sorts newest first and leaves the input alone', () => {
    const map = {
      a: { text: 'a', at: NOW - 10 },
      b: { text: 'b', at: NOW - 5 },
    };
    expect(Object.keys(keepFresh(map, NOW))).toEqual(['b', 'a']);
    expect(Object.keys(map)).toEqual(['a', 'b']);
  });
});
