import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  CanvasTerminal,
  DEFAULT_THEME,
  normalizeCanvasTerminalPaste,
  snapshotWithLocalInputOverlay,
  updateLocalInputOverlay,
  type LocalInputOverlay,
} from './canvas-terminal';

const EMPTY: LocalInputOverlay = { anchorLine: null, text: '' };

function createPasteEvent(text: string): Event {
  const event = new Event('paste', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'clipboardData', {
    value: {
      getData: (type: string) => (type === 'text/plain' ? text : ''),
    },
  });
  return event;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('CanvasTerminal local input overlay', () => {
  it('renders typed text at the current prompt line', () => {
    const overlay = updateLocalInputOverlay('ren@Mac % ', EMPTY, 'git');

    expect(snapshotWithLocalInputOverlay('ren@Mac % ', overlay)).toBe('ren@Mac % git');
  });

  it('keeps deliberate spaces visible immediately', () => {
    const overlay = updateLocalInputOverlay('ren@Mac % ', EMPTY, 'git status');

    expect(snapshotWithLocalInputOverlay('ren@Mac % ', overlay)).toBe('ren@Mac % git status');
  });

  it('erases one local CJK character on the first backspace after remote echo', () => {
    const typed = updateLocalInputOverlay('> ', EMPTY, '你好');
    const erased = updateLocalInputOverlay('> 你好', typed, '\x7f');

    expect(snapshotWithLocalInputOverlay('> 你好', erased)).toBe('> 你');
  });

  it('does not erase prompt text when there is no local input', () => {
    const overlay = updateLocalInputOverlay('> ', EMPTY, '\x7f');

    expect(snapshotWithLocalInputOverlay('> ', overlay)).toBe('> ');
  });

  it('clears local input on enter', () => {
    const typed = updateLocalInputOverlay('> ', EMPTY, 'git status');
    const submitted = updateLocalInputOverlay('> git status', typed, '\r');

    expect(snapshotWithLocalInputOverlay('> git status', submitted)).toBe('> git status');
  });
});

describe('normalizeCanvasTerminalPaste', () => {
  it('converts pasted line endings to terminal carriage returns', () => {
    expect(normalizeCanvasTerminalPaste('line1\nline2')).toBe('line1\rline2');
    expect(normalizeCanvasTerminalPaste('a\r\nb')).toBe('a\rb');
  });
});

describe('CanvasTerminal paste input', () => {
  it('emits normalized pasted text to data listeners', () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({
      fillRect: vi.fn(),
      fillText: vi.fn(),
      measureText: vi.fn(() => ({ width: 8 })),
      setTransform: vi.fn(),
      font: '',
      fillStyle: '',
      textBaseline: '',
    } as unknown as CanvasRenderingContext2D);

    const container = document.createElement('div');
    Object.defineProperty(container, 'getBoundingClientRect', {
      value: () => ({ width: 320, height: 180 }),
    });
    document.body.appendChild(container);

    const term = new CanvasTerminal(container, {
      cols: 80,
      rows: 24,
      fontFamily: 'monospace',
      fontSize: 14,
      scrollback: 100,
      theme: DEFAULT_THEME,
    });
    const received: string[] = [];
    term.onData((data) => received.push(data));

    container.dispatchEvent(createPasteEvent(''));
    const emptyPaste = received.length;

    container.dispatchEvent(createPasteEvent('git status\nnpm test'));

    expect(received.slice(emptyPaste)).toEqual(['git status\rnpm test']);

    term.dispose();
    container.remove();
  });
});
