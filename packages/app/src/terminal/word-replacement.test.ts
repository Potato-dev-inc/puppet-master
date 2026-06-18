import { describe, expect, it } from 'vitest';
import { buildReplacementInput, normalizeSuggestionText } from './word-replacement';

describe('normalizeSuggestionText', () => {
  it('dedupes duplicated mobile suggestion recovery text', () => {
    expect(normalizeSuggestionText('much much')).toBe('much');
    expect(normalizeSuggestionText('chocolate chocolate')).toBe('chocolate');
  });

  it('keeps ordinary text unchanged', () => {
    expect(normalizeSuggestionText('chocolate')).toBe('chocolate');
  });
});

describe('buildReplacementInput', () => {
  it('prepends backspaces for replaced characters', () => {
    expect(buildReplacementInput(4, 'much')).toBe('\x7f\x7f\x7f\x7fmuch');
  });

  it('returns only backspaces when insert text is empty', () => {
    expect(buildReplacementInput(2, '')).toBe('\x7f\x7f');
  });
});
