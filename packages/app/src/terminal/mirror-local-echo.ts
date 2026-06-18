import type { Terminal } from '@xterm/xterm';

/** Shells often echo this visual erase sequence for backspace. */
const BACKSPACE_ECHO_PATTERNS = ['\b \b', '\x08 \x08', '\b'];

export function isBackspaceKey(data: string): boolean {
  return data === '\x7f' || data === '\b';
}

export function isBackspaceEcho(chunk: string): boolean {
  if (!chunk) return true;
  return BACKSPACE_ECHO_PATTERNS.some((pattern) => chunk === pattern || chunk.startsWith(pattern));
}

/** Mirror viewers locally render outbound input while waiting for remote PTY echo. */
export function applyMirrorLocalEcho(terminal: Terminal, data: string): void {
  let printable = '';

  const flushPrintable = (): void => {
    if (!printable) return;
    terminal.write(printable);
    printable = '';
  };

  for (let index = 0; index < data.length; index += 1) {
    const ch = data[index];
    if (isBackspaceKey(ch)) {
      flushPrintable();
      terminal.write('\b \b');
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
