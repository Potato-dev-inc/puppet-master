import type { Terminal } from '@xterm/xterm';
import type { MirrorEchoFilter } from './mirror-echo-filter';

/** Shells often echo this visual erase sequence for backspace. */
const BACKSPACE_ECHO_PATTERNS = ['\b \b', '\x08 \x08', '\b'];

const EAST_ASIAN_WIDE =
  /[\u1100-\u115F\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE10-\uFE19\uFE30-\uFE6F\uFF00-\uFF60\uFFE0-\uFFE6]/;

/** Terminal column width for a single Unicode code point (xterm unicode11-style). */
export function terminalCellWidth(char: string): number {
  const codePoint = char.codePointAt(0);
  if (codePoint === undefined) return 0;
  if (codePoint < 32 || (codePoint >= 0x7f && codePoint < 0xa0)) return 0;
  if (EAST_ASIAN_WIDE.test(char)) return 2;
  return 1;
}

/** Visual erase sequence shells emit per terminal column. */
export const BACKSPACE_VISUAL_ERASE = '\b \b';

export function backspaceVisualErase(char: string): string {
  const width = terminalCellWidth(char);
  if (width <= 0) return '';
  return BACKSPACE_VISUAL_ERASE.repeat(width);
}

export function isBackspaceKey(data: string): boolean {
  return data === '\x7f' || data === '\b';
}

export function isBackspaceEcho(chunk: string): boolean {
  if (!chunk) return true;
  return BACKSPACE_ECHO_PATTERNS.some((pattern) => chunk === pattern || chunk.startsWith(pattern));
}

/** Mirror viewers locally render outbound input while waiting for remote PTY echo. */
export function applyMirrorLocalEcho(
  terminal: Terminal,
  data: string,
  filter?: MirrorEchoFilter,
): void {
  let printable = '';

  const flushPrintable = (): void => {
    if (!printable) return;
    if (filter) {
      filter.notePrintable(printable);
    }
    terminal.write(printable);
    printable = '';
  };

  for (let index = 0; index < data.length; index += 1) {
    const ch = data[index];
    if (isBackspaceKey(ch)) {
      flushPrintable();
      if (filter) {
        const erase = filter.noteBackspaceForEcho();
        if (erase) {
          terminal.write(erase);
        }
      } else {
        terminal.write(BACKSPACE_VISUAL_ERASE);
      }
      continue;
    }

    if (ch.charCodeAt(0) === 0x1b) {
      flushPrintable();
      terminal.input(data.slice(index), true);
      return;
    }

    printable += ch;
  }

  flushPrintable();
}
