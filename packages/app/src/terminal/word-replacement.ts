const BACKSPACE = '\x7f';

const CJK_CHARACTER =
  /\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul}/u;

const CJK_SPACING =
  /(\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul})\s+(?=\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul})/gu;

export const REPLACEMENT_INPUT_TYPES = new Set([
  'insertReplacementText',
  'insertFromComposition',
  'insertFromSuggestion',
  'insertCompositionText',
]);

export function isReplacementInputType(inputType: string): boolean {
  return REPLACEMENT_INPUT_TYPES.has(inputType);
}

/** Keyboards occasionally append a duplicated suggestion ("much much"). */
export function normalizeSuggestionText(insert: string): string {
  const trailingWhitespace = insert.match(/\s+$/u)?.[0] ?? '';
  const trimmed = trailingWhitespace
    ? insert.slice(0, -trailingWhitespace.length)
    : insert;
  const parts = trimmed.split(/\s+/);
  if (parts.length === 2 && parts[0] === parts[1]) {
    return parts[0] + trailingWhitespace;
  }
  return insert;
}

export function isCjkCharacter(char: string): boolean {
  if (!char) return false;
  return CJK_CHARACTER.test(char);
}

/**
 * Mobile IME keyboards often pad CJK phrases with spaces that shells do not echo.
 * Strip only CJK-to-CJK padding so deliberate separators like "你好 world" survive.
 */
export function stripCjkImeSpaces(text: string): string {
  return text.replace(CJK_SPACING, '$1');
}

/** Drop a leading IME space when appending the next CJK phrase. */
export function stripCjkImeLeadingSpace(previous: string, inserted: string): string {
  if (!inserted.startsWith(' ')) return inserted;
  const lastPrevious = Array.from(previous).at(-1);
  const firstInserted = Array.from(inserted.trimStart()).at(0);
  if (
    lastPrevious &&
    isCjkCharacter(lastPrevious) &&
    firstInserted &&
    isCjkCharacter(firstInserted)
  ) {
    return inserted.trimStart();
  }
  return inserted;
}

/** PTY backspaces followed by the replacement text. */
export function buildReplacementInput(replacedLength: number, insertText: string): string {
  if (replacedLength <= 0 && insertText.length === 0) return '';
  if (replacedLength <= 0) return insertText;
  if (insertText.length === 0) return BACKSPACE.repeat(replacedLength);
  return BACKSPACE.repeat(replacedLength) + insertText;
}
