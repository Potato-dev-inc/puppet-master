import { describe, expect, it } from 'vitest';
import {
  configureDesktopXtermTextarea,
  isTerminalPasteShortcut,
  normalizeTerminalPaste,
} from './xterm-textarea';

describe('normalizeTerminalPaste', () => {
  it('converts newlines to carriage returns', () => {
    expect(normalizeTerminalPaste('line1\nline2')).toBe('line1\rline2');
    expect(normalizeTerminalPaste('a\r\nb')).toBe('a\rb');
  });
});

describe('configureDesktopXtermTextarea', () => {
  it('covers the terminal host for clipboard focus', () => {
    const host = document.createElement('div');
    const textarea = document.createElement('textarea');
    host.appendChild(textarea);
    document.body.appendChild(host);

    configureDesktopXtermTextarea(host, textarea);

    expect(host.style.position).toBe('relative');
    expect(textarea.style.position).toBe('absolute');
    expect(textarea.style.width).toBe('100%');
    expect(textarea.style.height).toBe('100%');
    expect(textarea.style.opacity).toBe('0.01');

    host.remove();
  });
});

describe('isTerminalPasteShortcut', () => {
  it('accepts Ctrl+V and Cmd+V without Alt', () => {
    expect(isTerminalPasteShortcut({ key: 'v', ctrlKey: true, metaKey: false, altKey: false })).toBe(true);
    expect(isTerminalPasteShortcut({ key: 'V', ctrlKey: false, metaKey: true, altKey: false })).toBe(true);
  });

  it('rejects non-paste and Alt-modified shortcuts', () => {
    expect(isTerminalPasteShortcut({ key: 'c', ctrlKey: true, metaKey: false, altKey: false })).toBe(false);
    expect(isTerminalPasteShortcut({ key: 'v', ctrlKey: true, metaKey: false, altKey: true })).toBe(false);
    expect(isTerminalPasteShortcut({ key: 'v', ctrlKey: false, metaKey: false, altKey: false })).toBe(false);
  });
});
