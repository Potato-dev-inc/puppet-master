import { describe, expect, it } from 'vitest';
import {
  applyMirrorLocalEcho,
  isBackspaceEcho,
  isBackspaceKey,
} from './mirror-local-echo';

describe('mirror-local-echo', () => {
  it('detects backspace keys and echo sequences', () => {
    expect(isBackspaceKey('\x7f')).toBe(true);
    expect(isBackspaceKey('\b')).toBe(true);
    expect(isBackspaceKey('a')).toBe(false);
    expect(isBackspaceEcho('\b \b')).toBe(true);
    expect(isBackspaceEcho('hello')).toBe(false);
  });

  it('renders mixed backspace and printable payloads locally', () => {
    const calls: string[] = [];
    const terminal = {
      input: (data: string) => calls.push(`input:${data}`),
      write: (data: string) => calls.push(`write:${data}`),
    } as unknown as import('@xterm/xterm').Terminal;

    applyMirrorLocalEcho(terminal, '\x7f\x7fmuch');

    expect(calls).toEqual(['write:\b \b', 'write:\b \b', 'write:much']);
  });

  it('does not print escape sequence suffixes as visible text', () => {
    const calls: string[] = [];
    const terminal = {
      input: (data: string) => calls.push(`input:${data}`),
      write: (data: string) => calls.push(`write:${data}`),
    } as unknown as import('@xterm/xterm').Terminal;

    applyMirrorLocalEcho(terminal, '\x1b[A');

    expect(calls).toEqual(['input:\x1b[A']);
  });
});
