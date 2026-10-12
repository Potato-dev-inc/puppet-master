import { describe, expect, it } from 'vitest';
import { MirrorEchoFilter } from './mirror-echo-filter';
import {
  isTerminalReport,
  applyMirrorLocalEcho,
  BACKSPACE_VISUAL_ERASE,
  backspaceVisualErase,
  isBackspaceEcho,
  isBackspaceKey,
  terminalCellWidth,
} from './mirror-local-echo';

describe('mirror-local-echo', () => {
  it('detects backspace keys and echo sequences', () => {
    expect(isBackspaceKey('\x7f')).toBe(true);
    expect(isBackspaceKey('\b')).toBe(true);
    expect(isBackspaceKey('a')).toBe(false);
    expect(isBackspaceEcho('\b \b')).toBe(true);
    expect(isBackspaceEcho('hello')).toBe(false);
    expect(terminalCellWidth('你')).toBe(2);
    expect(backspaceVisualErase('你')).toBe(BACKSPACE_VISUAL_ERASE.repeat(2));
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

  it('does not erase screen content when filter has no pending local echo', () => {
    const calls: string[] = [];
    const terminal = {
      input: (data: string) => calls.push(`input:${data}`),
      write: (data: string) => calls.push(`write:${data}`),
    } as unknown as import('@xterm/xterm').Terminal;
    const filter = new MirrorEchoFilter();

    applyMirrorLocalEcho(terminal, '\x7f\x7f', filter);

    expect(calls).toEqual([]);
    expect(filter.shouldSkipInbound(new TextEncoder().encode('\b \b'))).toBe(false);
  });

  it('erases only locally echoed chars when filter is provided', () => {
    const calls: string[] = [];
    const terminal = {
      input: (data: string) => calls.push(`input:${data}`),
      write: (data: string) => calls.push(`write:${data}`),
    } as unknown as import('@xterm/xterm').Terminal;
    const filter = new MirrorEchoFilter();

    applyMirrorLocalEcho(terminal, 'ab\x7f', filter);

    expect(calls).toEqual(['write:ab', `write:${BACKSPACE_VISUAL_ERASE}`]);
  });

  it('erases wide characters with two column backspaces', () => {
    const calls: string[] = [];
    const terminal = {
      input: (data: string) => calls.push(`input:${data}`),
      write: (data: string) => calls.push(`write:${data}`),
    } as unknown as import('@xterm/xterm').Terminal;
    const filter = new MirrorEchoFilter();

    applyMirrorLocalEcho(terminal, '你\x7f', filter);

    expect(calls).toEqual(['write:你', `write:${BACKSPACE_VISUAL_ERASE.repeat(2)}`]);
  });

  it('tells xterm query replies apart from typed keys', () => {
    for (const report of ['\x1b[12;40R', '\x1b[?1;2c', '\x1b[>0;276;0c', '\x1b[?2026;2$y', '\x1b]11;rgb:0000/0000/0000\x1b\\','\x1b]10;rgb:ffff/ffff/ffff\x07', '\x1b[I', '\x1b[O', '\x1b[?1;2c\x1b[12;40R']) {
      expect(isTerminalReport(report), JSON.stringify(report)).toBe(true);
    }
    for (const key of ['a', '\r', '\x1b', '\x1b[A', '\x1b[B', '\x1b[3~', '\x1b[1;5C', 'hello', '\x7f']) {
      expect(isTerminalReport(key), JSON.stringify(key)).toBe(false);
    }
  });
});
