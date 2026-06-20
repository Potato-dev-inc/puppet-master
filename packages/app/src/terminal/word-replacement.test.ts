import { describe, expect, it } from 'vitest';
import {
  buildReplacementInput,
  normalizeSuggestionText,
  stripCjkImeLeadingSpace,
  stripCjkImeSpaces,
} from './word-replacement';

describe('normalizeSuggestionText', () => {
  it('dedupes duplicated mobile suggestion recovery text', () => {
    expect(normalizeSuggestionText('much much')).toBe('much');
    expect(normalizeSuggestionText('chocolate chocolate')).toBe('chocolate');
  });

  it('keeps ordinary text and suggestion separators unchanged', () => {
    expect(normalizeSuggestionText('chocolate')).toBe('chocolate');
    expect(normalizeSuggestionText('chocolate ')).toBe('chocolate ');
    expect(normalizeSuggestionText('much much ')).toBe('much ');
  });

  it('strips IME spaces between CJK phrases', () => {
    expect(stripCjkImeSpaces('你好 世界')).toBe('你好世界');
  });

  it('keeps deliberate trailing and latin/CJK spaces', () => {
    expect(stripCjkImeSpaces('你好 ')).toBe('你好 ');
    expect(stripCjkImeSpaces('你好 world')).toBe('你好 world');
    expect(stripCjkImeSpaces('git 状态')).toBe('git 状态');
  });

  it('drops leading IME space when appending CJK text', () => {
    expect(stripCjkImeLeadingSpace('你好', ' 世界')).toBe('世界');
    expect(stripCjkImeLeadingSpace('git', ' status')).toBe(' status');
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
