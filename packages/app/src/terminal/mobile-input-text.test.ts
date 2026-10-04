import { describe, expect, it } from 'vitest';
import { buildInputDelta } from './mobile-input-text';

describe('buildInputDelta', () => {
  it('appends ordinary typing', () => {
    expect(buildInputDelta('suf', 'sufh')).toBe('h');
  });

  it('replaces unrelated autocorrect text', () => {
    expect(buildInputDelta('sufh', 'much')).toBe('\x7f\x7f\x7f\x7fmuch');
  });

  it('replaces a longer mistyped word with a suggestion', () => {
    expect(buildInputDelta('lsdjfl', 'chocolate')).toBe('\x7f\x7f\x7f\x7f\x7f\x7fchocolate');
  });

  it('only sends the completed suffix for prefix completions', () => {
    expect(buildInputDelta('cou', 'could')).toBe('ld');
  });

  it('normalizes pasted line endings for terminal input', () => {
    expect(buildInputDelta('', 'echo hi\n')).toBe('echo hi\r');
  });

  it('drops IME spaces between CJK phrases', () => {
    expect(buildInputDelta('你好', '你好 世界')).toBe('世界');
  });

  it('keeps deliberate spaces after CJK before latin text', () => {
    expect(buildInputDelta('你好', '你好 world')).toBe(' world');
  });
});
