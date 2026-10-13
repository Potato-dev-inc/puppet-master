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

/** One key on the special-keys panel. `label` is the accessible name. */
export interface SpecialKey {
  id: string;
  /** Large symbol shown on the key cap. */
  glyph: string;
  /** Small word under the glyph. */
  caption?: string;
  /** Bytes sent to the PTY (empty for the Ctrl latch, which sends nothing by itself). */
  value: string;
  label: string;
  /** Holding the key repeats it. */
  repeat?: boolean;
  /** Spans two columns of its section grid. */
  wide?: boolean;
  /** Toggles the Ctrl modifier for the next typed letter instead of sending bytes. */
  latch?: boolean;
}

export interface SpecialKeyGroup {
  id: string;
  title: string;
  /** Keys in reading order; the group's grid lays them out in rows of its column count. */
  keys: SpecialKey[];
}

export const SPECIAL_KEY_GROUPS: SpecialKeyGroup[] = [
  {
    id: 'control',
    title: 'Control',
    keys: [
      { id: 'esc', glyph: '⎋', caption: 'esc', value: CONTROL_KEYS.Escape, label: 'Escape' },
      { id: 'tab', glyph: '⇥', caption: 'tab', value: CONTROL_KEYS.Tab, label: 'Tab' },
      // Claude Code cycles its permission modes with Shift-Tab.
      { id: 'backtab', glyph: '⇤', caption: 'mode', value: '\x1b[Z', label: 'Shift-Tab' },
      { id: 'ctrl-c', glyph: '⌃C', caption: 'interrupt', value: '\x03', label: 'Ctrl-C' },
      { id: 'ctrl-d', glyph: '⌃D', caption: 'eof', value: '\x04', label: 'Ctrl-D' },
      { id: 'ctrl-l', glyph: '⌃L', caption: 'clear', value: '\x0c', label: 'Ctrl-L' },
      { id: 'ctrl', glyph: '⌃', caption: 'ctrl', value: '', label: 'Control modifier', latch: true },
    ],
  },
  {
    id: 'navigate',
    title: 'Navigate',
    // Laid out as: Home ↑ End / ← ↓ → (the arrows form an inverted T).
    keys: [
      { id: 'home', glyph: '⇱', caption: 'home', value: CONTROL_KEYS.Home, label: 'Home' },
      { id: 'up', glyph: '↑', value: CONTROL_KEYS.ArrowUp, label: 'Up arrow', repeat: true },
      { id: 'end', glyph: '⇲', caption: 'end', value: CONTROL_KEYS.End, label: 'End' },
      { id: 'left', glyph: '←', value: CONTROL_KEYS.ArrowLeft, label: 'Left arrow', repeat: true },
      { id: 'down', glyph: '↓', value: CONTROL_KEYS.ArrowDown, label: 'Down arrow', repeat: true },
      { id: 'right', glyph: '→', value: CONTROL_KEYS.ArrowRight, label: 'Right arrow', repeat: true },
    ],
  },
  {
    id: 'edit',
    title: 'Edit',
    keys: [
      { id: 'backspace', glyph: '⌫', caption: 'delete', value: BACKSPACE, label: 'Backspace', repeat: true },
      { id: 'forward-delete', glyph: '⌦', caption: 'forward', value: CONTROL_KEYS.Delete, label: 'Forward delete', repeat: true },
      { id: 'return', glyph: '↵', caption: 'return', value: ENTER, label: 'Return', wide: true },
    ],
  },
];

/** Every key of every group, in panel order. */
export const SPECIAL_KEYS: SpecialKey[] = SPECIAL_KEY_GROUPS.flatMap((group) => group.keys);

/** Ctrl latch: the next typed letter (or one of @[\]^_) is sent as its control code. */
export function controlCodeFor(letter: string): string | null {
  if (letter.length !== 1 || !/^[a-zA-Z@[\\\]^_]$/.test(letter)) return null;
  return String.fromCharCode(letter.toUpperCase().charCodeAt(0) & 0x1f);
}

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
