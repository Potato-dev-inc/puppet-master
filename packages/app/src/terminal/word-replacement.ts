const BACKSPACE = '\x7f';

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
  const trimmed = insert.replace(/\s+$/, '');
  const parts = trimmed.split(/\s+/);
  if (parts.length === 2 && parts[0] === parts[1]) {
    return parts[0];
  }
  return insert;
}

/** PTY backspaces followed by the replacement text. */
export function buildReplacementInput(replacedLength: number, insertText: string): string {
  if (replacedLength <= 0 && insertText.length === 0) return '';
  if (replacedLength <= 0) return insertText;
  if (insertText.length === 0) return BACKSPACE.repeat(replacedLength);
  return BACKSPACE.repeat(replacedLength) + insertText;
}
