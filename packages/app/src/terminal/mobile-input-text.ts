// Pure keystroke/text helpers for the mobile terminal input: key byte sequences, the on-screen special-key
// row, and the diff that turns edits of the hidden input box into PTY input.
import { stripCjkImeLeadingSpace } from './word-replacement';

export const BACKSPACE = '\x7f';
export const ENTER = '\r';

export const CONTROL_KEYS: Record<string, string> = {
  ArrowUp: '\x1b[A',
  ArrowDown: '\x1b[B',
  ArrowRight: '\x1b[C',
  ArrowLeft: '\x1b[D',
  Delete: '\x1b[3~',
  Home: '\x1b[H',
  End: '\x1b[F',
  Escape: '\x1b',
  Tab: '\t',
};

export const SPECIAL_KEYS: Array<{ label: string; value: string; title: string }> = [
  { label: 'Esc', value: CONTROL_KEYS.Escape, title: 'Escape' },
  { label: 'Ctrl-C', value: '\x03', title: 'Interrupt / cancel current command' },
  { label: 'Ctrl-D', value: '\x04', title: 'End of input / exit shell' },
  { label: 'Ctrl-Z', value: '\x1a', title: 'Suspend process' },
  { label: 'Ctrl-L', value: '\x0c', title: 'Clear screen' },
  { label: 'Tab', value: CONTROL_KEYS.Tab, title: 'Tab completion' },
  { label: 'Enter', value: ENTER, title: 'Enter' },
  { label: 'Back', value: BACKSPACE, title: 'Backspace' },
  { label: 'Del', value: CONTROL_KEYS.Delete, title: 'Delete' },
  { label: '←', value: CONTROL_KEYS.ArrowLeft, title: 'Left arrow' },
  { label: '↑', value: CONTROL_KEYS.ArrowUp, title: 'Up arrow' },
  { label: '↓', value: CONTROL_KEYS.ArrowDown, title: 'Down arrow' },
  { label: '→', value: CONTROL_KEYS.ArrowRight, title: 'Right arrow' },
  { label: 'Home', value: CONTROL_KEYS.Home, title: 'Home' },
  { label: 'End', value: CONTROL_KEYS.End, title: 'End' },
];

function normalizeTerminalLineEndings(text: string): string {
  return text.replace(/\r\n/g, ENTER).replace(/\n/g, ENTER);
}

export function isDeletionOnlyPayload(text: string): boolean {
  return text.length > 0 && [...text].every((ch) => ch === BACKSPACE);
}

export function endsWithInsertedWhitespace(previous: string, next: string): boolean {
  if (next.length <= previous.length) return false;
  return /\s$/u.test(next.slice(previous.length));
}

/** Translate committed vs current input text into PTY keystrokes (used only on flush). */
export function buildInputDelta(previous: string, next: string): string {
  if (previous === next) return '';

  let prefixLength = 0;
  const maxPrefix = Math.min(previous.length, next.length);
  while (
    prefixLength < maxPrefix &&
    previous[prefixLength] === next[prefixLength]
  ) {
    prefixLength += 1;
  }

  const deletedLength = previous.length - prefixLength;
  const inserted = stripCjkImeLeadingSpace(
    previous.slice(0, prefixLength),
    next.slice(prefixLength),
  );
  return BACKSPACE.repeat(deletedLength) + normalizeTerminalLineEndings(inserted);
}
