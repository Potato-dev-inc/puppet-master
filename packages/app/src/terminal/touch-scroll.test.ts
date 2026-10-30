// @vitest-environment jsdom
import type { Terminal } from '@xterm/xterm';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  linesForDelta,
  TouchScrollController,
  wheelSequence,
} from './touch-scroll';

interface Point {
  identifier: number;
  clientX: number;
  clientY: number;
}

function fireTouch(target: HTMLElement, type: string, points: Point[]): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'touches', { value: points });
  target.dispatchEvent(event);
  return event;
}

function mockTerminal(mouseTrackingMode: string, bufferType: 'normal' | 'alternate') {
  const terminal = {
    modes: { mouseTrackingMode },
    buffer: { active: { type: bufferType, viewportY: 0, baseY: 0 } },
    cols: 80,
    rows: 24,
    scrollLines: vi.fn(),
    scrollToBottom: vi.fn(),
    onScroll: vi.fn((_listener: () => void) => ({ dispose: vi.fn() })),
    onWriteParsed: vi.fn(() => ({ dispose: vi.fn() })),
  };
  return terminal;
}

describe('wheelSequence', () => {
  it('encodes SGR wheel up and down with the cell position', () => {
    expect(wheelSequence('up', 5, 7, true)).toBe('\x1b[<64;5;7M');
    expect(wheelSequence('down', 5, 7, true)).toBe('\x1b[<65;5;7M');
  });

  it('encodes legacy X10 wheel reports as offset bytes', () => {
    expect(wheelSequence('up', 5, 7, false)).toBe(`\x1b[M${String.fromCharCode(96)}${String.fromCharCode(37)}${String.fromCharCode(39)}`);
    expect(wheelSequence('down', 1, 1, false)).toBe(`\x1b[M${String.fromCharCode(97)}${String.fromCharCode(33)}${String.fromCharCode(33)}`);
  });
});

describe('linesForDelta', () => {
  it('returns whole rows and keeps the remainder', () => {
    expect(linesForDelta(45, 20)).toEqual({ lines: 2, remainderPx: 5 });
    expect(linesForDelta(-30, 20)).toEqual({ lines: -1, remainderPx: -10 });
    expect(linesForDelta(7, 20)).toEqual({ lines: 0, remainderPx: 7 });
  });

  it('returns no lines for an unknown cell size', () => {
    expect(linesForDelta(50, 0)).toEqual({ lines: 0, remainderPx: 50 });
  });
});

describe('TouchScrollController', () => {
  let controller: TouchScrollController | null = null;
  let container: HTMLElement;

  afterEach(() => {
    controller?.dispose();
    controller = null;
    container?.remove();
  });

  function create(terminal: ReturnType<typeof mockTerminal>, sendInput = vi.fn(), onScrollStart = vi.fn()) {
    container = document.createElement('div');
    document.body.appendChild(container);
    controller = new TouchScrollController(container, terminal as unknown as Terminal, {
      sendInput,
      cellHeight: () => 20,
      cellFromPoint: () => ({ col: 5, row: 7 }),
      onScrollStart,
    });
    return { sendInput, onScrollStart };
  }

  it('scrolls the normal buffer by rows; a finger moving down reveals older output', () => {
    const terminal = mockTerminal('none', 'normal');
    const { onScrollStart } = create(terminal);

    const start = fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    expect(start.defaultPrevented).toBe(false);
    const move = fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 140 }]);
    expect(move.defaultPrevented).toBe(true);
    expect(terminal.scrollLines).toHaveBeenCalledWith(-2);
    expect(onScrollStart).toHaveBeenCalledTimes(1);
    fireTouch(container, 'touchend', []);

    fireTouch(container, 'touchstart', [{ identifier: 2, clientX: 100, clientY: 200 }]);
    fireTouch(container, 'touchmove', [{ identifier: 2, clientX: 100, clientY: 170 }]);
    expect(terminal.scrollLines).toHaveBeenLastCalledWith(1);
    fireTouch(container, 'touchend', []);
  });

  it('sends SGR wheel events to a mouse-tracking agent instead of scrolling locally', () => {
    const terminal = mockTerminal('any', 'alternate');
    const { sendInput } = create(terminal);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 140 }]);
    fireTouch(container, 'touchend', []);

    expect(terminal.scrollLines).not.toHaveBeenCalled();
    expect(sendInput.mock.calls).toEqual([['\x1b[<64;5;7M'], ['\x1b[<64;5;7M']]);
  });

  it('does nothing for the alternate buffer without mouse tracking', () => {
    const terminal = mockTerminal('none', 'alternate');
    const { sendInput } = create(terminal);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    fireTouch(container, 'touchend', []);

    expect(terminal.scrollLines).not.toHaveBeenCalled();
    expect(sendInput).not.toHaveBeenCalled();
  });

  it('ignores a mostly horizontal drag and leaves it to the browser', () => {
    const terminal = mockTerminal('none', 'normal');
    create(terminal);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    const move = fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 180, clientY: 110 }]);
    fireTouch(container, 'touchend', []);

    expect(move.defaultPrevented).toBe(false);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
  });

  it('shows the scroll-to-bottom button only when the normal buffer is scrolled up', () => {
    const terminal = mockTerminal('none', 'normal');
    create(terminal);
    const button = container.querySelector('.terminal-scroll-bottom') as HTMLButtonElement;
    expect(button.classList.contains('terminal-scroll-bottom--visible')).toBe(false);

    terminal.buffer.active.viewportY = 10;
    terminal.buffer.active.baseY = 50;
    const onScroll = terminal.onScroll.mock.calls[0][0] as () => void;
    onScroll();
    expect(button.classList.contains('terminal-scroll-bottom--visible')).toBe(true);

    button.click();
    expect(terminal.scrollToBottom).toHaveBeenCalledTimes(1);
  });

  it('removes its button and listeners on dispose', () => {
    const terminal = mockTerminal('none', 'normal');
    create(terminal);
    controller?.dispose();
    controller = null;
    expect(container.querySelector('.terminal-scroll-bottom')).toBeNull();

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
  });
});

describe('TouchScrollController viewport pan', () => {
  let controller: TouchScrollController | null = null;
  let container: HTMLElement;

  afterEach(() => {
    controller?.dispose();
    controller = null;
    container?.remove();
  });

  function create(
    terminal: ReturnType<typeof mockTerminal>,
    panViewport: (dy: number) => number,
    extra: { isPanned?: () => boolean; resetPan?: () => void } = {},
  ) {
    container = document.createElement('div');
    document.body.appendChild(container);
    controller = new TouchScrollController(container, terminal as unknown as Terminal, {
      sendInput: vi.fn(),
      cellHeight: () => 20,
      cellFromPoint: () => ({ col: 5, row: 7 }),
      panViewport,
      ...extra,
    });
  }

  it('pans first when the finger moves down and scrolls xterm with what the pan did not take', () => {
    const terminal = mockTerminal('none', 'normal');
    const panViewport = vi.fn((dy: number) => Math.max(0, dy - 30));
    create(terminal, panViewport);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 140 }]);
    expect(panViewport).toHaveBeenCalledWith(40);
    // 40px minus the 30px the pan took leaves 10px: no whole row yet.
    expect(terminal.scrollLines).not.toHaveBeenCalled();
    fireTouch(container, 'touchend', []);
  });

  it('does not scroll xterm at all while the pan takes the whole drag', () => {
    const terminal = mockTerminal('none', 'normal');
    create(terminal, () => 0);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
    fireTouch(container, 'touchend', []);
  });

  it('un-pans before moving the finger up once xterm is at its bottom', () => {
    const terminal = mockTerminal('none', 'normal');
    // The pan takes the whole upward drag (it un-pans), so nothing is left to scroll xterm with.
    const panViewport = vi.fn(() => 0);
    create(terminal, panViewport);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 200 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    expect(panViewport).toHaveBeenCalledWith(-40);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
    fireTouch(container, 'touchend', []);
  });

  it('scrolls xterm back toward its bottom before un-panning', () => {
    const terminal = mockTerminal('none', 'normal');
    terminal.buffer.active.viewportY = 10;
    terminal.buffer.active.baseY = 50;
    const panViewport = vi.fn((dy: number) => dy);
    create(terminal, panViewport);

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 200 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    expect(terminal.scrollLines).toHaveBeenCalledWith(2);
    expect(panViewport).not.toHaveBeenCalled();
    fireTouch(container, 'touchend', []);
  });

  it('pans a mouse-tracking agent before sending wheel events for the rest', () => {
    const terminal = mockTerminal('vt200', 'normal');
    const sendInput = vi.fn();
    container = document.createElement('div');
    document.body.appendChild(container);
    controller = new TouchScrollController(container, terminal as unknown as Terminal, {
      sendInput,
      cellHeight: () => 20,
      cellFromPoint: () => ({ col: 5, row: 7 }),
      panViewport: (dy) => Math.max(0, dy - 20),
    });

    fireTouch(container, 'touchstart', [{ identifier: 1, clientX: 100, clientY: 100 }]);
    fireTouch(container, 'touchmove', [{ identifier: 1, clientX: 100, clientY: 160 }]);
    // 60px down, 20px taken by the pan: 40px is two wheel steps.
    expect(sendInput).toHaveBeenCalledTimes(2);
    fireTouch(container, 'touchend', []);
  });

  it('shows the jump-to-bottom button while panned and resets the pan when tapped', () => {
    const terminal = mockTerminal('none', 'normal');
    let panned = true;
    const resetPan = vi.fn(() => {
      panned = false;
    });
    create(terminal, () => 0, { isPanned: () => panned, resetPan });
    const button = container.querySelector('.terminal-scroll-bottom') as HTMLButtonElement;
    const onScroll = terminal.onScroll.mock.calls[0][0] as () => void;
    onScroll();
    expect(button.classList.contains('terminal-scroll-bottom--visible')).toBe(true);

    button.click();
    expect(resetPan).toHaveBeenCalledTimes(1);
    expect(terminal.scrollToBottom).toHaveBeenCalledTimes(1);
  });
});
