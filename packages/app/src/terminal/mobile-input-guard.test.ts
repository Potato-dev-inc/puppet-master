import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  buildInputDelta,
  DEFAULT_MOBILE_BUFFER_MS,
  MobileInputGuard,
  MOBILE_INPUT_ENGAGED_CLASS,
  MOBILE_INPUT_HIDDEN_CLASS,
  MOBILE_LONG_PRESS_MS,
  type MobileInputDelivery,
} from './mobile-input-guard';
import { buildReplacementInput } from './word-replacement';

const TEST_BUFFER_MS = 25;

describe('buildReplacementInput', () => {
  it('prepends backspaces for replaced characters', () => {
    expect(buildReplacementInput(3, 'could')).toBe('\x7f\x7f\x7f' + 'could');
  });
});

describe('buildInputDelta', () => {
  it('appends ordinary typing', () => {
    expect(buildInputDelta('suf', 'sufh')).toBe('h');
  });

  it('replaces unrelated autocorrect text', () => {
    expect(buildInputDelta('sufh', 'much')).toBe('\x7f\x7f\x7f\x7fmuch');
  });

  it('replaces a longer mistyped word with a suggestion', () => {
    expect(buildInputDelta('lsdjfl', 'chocolate')).toBe('\x7f\x7f\x7f\x7f\x7f\x7fchocolate');
  });

  it('only sends the completed suffix for prefix completions', () => {
    expect(buildInputDelta('cou', 'could')).toBe('ld');
  });

  it('normalizes pasted line endings for terminal input', () => {
    expect(buildInputDelta('', 'echo hi\n')).toBe('echo hi\r');
  });
});

describe('MobileInputGuard', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    if (typeof PointerEvent === 'undefined') {
      class PointerEventPolyfill extends MouseEvent {
        readonly pointerId: number;
        readonly pointerType: string;
        constructor(type: string, params: PointerEventInit = {}) {
          super(type, params);
          this.pointerId = params.pointerId ?? 0;
          this.pointerType = params.pointerType ?? '';
        }
      }
      vi.stubGlobal('PointerEvent', PointerEventPolyfill);
    }
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function createGuard(bufferDelayMs = TEST_BUFFER_MS, inputVisible?: boolean) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const emitted: Array<{ text: string; delivery: MobileInputDelivery }> = [];
    const buffer: string[] = [];
    const scrollToCursor = vi.fn();
    const guard = new MobileInputGuard({
      container,
      bufferDelayMs,
      inputVisible,
      emitInput: (text, delivery) => emitted.push({ text, delivery }),
      scrollToCursor,
      onBufferChange: (text) => buffer.push(text),
    });
    const input = (
      container.querySelector('[data-mobile-terminal-input="true"]') ??
      document.querySelector('.terminal-mobile-keyboard-sink-zone [data-mobile-terminal-input="true"]')
    ) as HTMLTextAreaElement;
    const form = (
      container.querySelector('form.terminal-mobile-command-form') ??
      document.querySelector('.terminal-mobile-keyboard-sink-zone form.terminal-mobile-command-form')
    ) as HTMLFormElement;

    return {
      buffer,
      cleanup: () => {
        guard.dispose();
        container.remove();
      },
      container,
      emitted,
      form,
      guard,
      input,
      scrollToCursor,
    };
  }

  function dispatchInput(input: HTMLTextAreaElement, value: string, inputType: string): void {
    input.value = value;
    input.dispatchEvent(
      new InputEvent('input', {
        bubbles: true,
        data: value,
        inputType,
      }),
    );
  }

  it('defaults to a short memory buffer', () => {
    expect(DEFAULT_MOBILE_BUFFER_MS).toBe(250);
  });

  it('uses a textarea command field inside a form', () => {
    const { cleanup, form, input } = createGuard();

    expect(form).toBeTruthy();
    expect(input.tagName).toBe('TEXTAREA');
    expect(input.getAttribute('autocorrect')).toBe('on');
    cleanup();
  });

  it('can hide the mobile command field while keeping the input mounted', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);

    expect(container.classList.contains(MOBILE_INPUT_HIDDEN_CLASS)).toBe(true);
    expect(input.isConnected).toBe(true);
    expect(input.closest('.terminal-mobile-keyboard-sink-zone')).toBeTruthy();
    expect(container.querySelector('[data-mobile-terminal-input="true"]')).toBeNull();
    cleanup();
    expect(container.classList.contains(MOBILE_INPUT_HIDDEN_CLASS)).toBe(false);
  });


  it('holds typing in the input field until the buffer timer expires', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 's', 'insertText');
    dispatchInput(input, 'su', 'insertText');

    expect(emitted).toEqual([]);
    expect(input.value).toBe('su');

    vi.advanceTimersByTime(TEST_BUFFER_MS - 1);
    expect(emitted).toEqual([]);

    vi.advanceTimersByTime(1);
    expect(emitted).toEqual([{ text: 'su', delivery: 'immediate' }]);
    cleanup();
  });

  it('flushes typing immediately when the buffer is disabled', () => {
    const { cleanup, emitted, input } = createGuard(0);

    dispatchInput(input, 'l', 'insertText');

    expect(emitted).toEqual([{ text: 'l', delivery: 'immediate' }]);
    cleanup();
  });

  it('extends the buffer timer when more characters are typed', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'h', 'insertText');
    vi.advanceTimersByTime(20);
    dispatchInput(input, 'hi', 'insertText');
    vi.advanceTimersByTime(20);
    expect(emitted).toEqual([]);

    vi.advanceTimersByTime(5);
    expect(emitted).toEqual([{ text: 'hi', delivery: 'immediate' }]);
    cleanup();
  });

  it('commits autocorrect replacement after the buffer settles', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'sufh', 'insertText');
    dispatchInput(input, 'much', 'insertReplacementText');
    expect(emitted).toEqual([]);

    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted).toEqual([{ text: 'much', delivery: 'immediate' }]);
    expect(input.value).toBe('much');
    cleanup();
  });

  it('dedupes replacement recovery text from mobile keyboards', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'sufh', 'insertText');
    dispatchInput(input, 'much much', 'insertReplacementText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);

    expect(emitted).toEqual([{ text: 'much', delivery: 'immediate' }]);
    cleanup();
  });

  it('sends backspaces when autocorrect replaces text already committed to the terminal', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'sufh', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    dispatchInput(input, 'much', 'insertReplacementText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);

    expect(emitted).toEqual([
      { text: 'sufh', delivery: 'immediate' },
      { text: '\x7f\x7f\x7f\x7fmuch', delivery: 'immediate' },
    ]);
    cleanup();
  });

  it('submits the form on Enter and clears the field', () => {
    const { cleanup, emitted, form, input } = createGuard();

    dispatchInput(input, 'ls', 'insertText');
    form.requestSubmit();

    expect(emitted).toEqual([
      { text: 'ls', delivery: 'immediate' },
      { text: '\r', delivery: 'immediate' },
    ]);
    expect(input.value).toBe('');
    cleanup();
  });

  it('buffers backspace edits until the timer flushes them to the terminal', () => {
    const { cleanup, emitted, input, scrollToCursor } = createGuard();
    dispatchInput(input, 'abcd', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted).toEqual([{ text: 'abcd', delivery: 'immediate' }]);
    scrollToCursor.mockClear();

    dispatchInput(input, 'abc', 'deleteContentBackward');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted.at(-1)).toEqual({ text: '\x7f', delivery: 'immediate' });
    expect(scrollToCursor).not.toHaveBeenCalled();
    cleanup();
  });

  it('sends terminal backspace when the input field is already empty', () => {
    const { cleanup, emitted, input, scrollToCursor } = createGuard();

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true }));

    expect(emitted).toEqual([{ text: '\x7f', delivery: 'immediate' }]);
    expect(scrollToCursor).not.toHaveBeenCalled();
    cleanup();
  });

  it('keeps the full command line in the input field across words', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'git ', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    dispatchInput(input, 'git status', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);

    expect(emitted).toEqual([
      { text: 'git ', delivery: 'immediate' },
      { text: 'status', delivery: 'immediate' },
    ]);
    expect(input.value).toBe('git status');
    cleanup();
  });

  function dispatchPointerPress(
    container: HTMLElement,
    durationMs: number,
    clientX = 50,
    clientY = 50,
    target: HTMLElement = container,
  ): void {
    target.dispatchEvent(
      new PointerEvent('pointerdown', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY,
        pointerId: 1,
        pointerType: 'touch',
      }),
    );
    if (durationMs > 0) {
      vi.advanceTimersByTime(durationMs);
    }
    target.dispatchEvent(
      new PointerEvent('pointerup', {
        bubbles: true,
        cancelable: true,
        clientX,
        clientY,
        pointerId: 1,
        pointerType: 'touch',
      }),
    );
  }

  function dispatchShortTap(container: HTMLElement): void {
    dispatchPointerPress(container, MOBILE_LONG_PRESS_MS - 1);
  }

  function dispatchLongPress(container: HTMLElement): void {
    dispatchPointerPress(container, MOBILE_LONG_PRESS_MS);
  }

  it('focuses the input after a long press on the terminal area', () => {
    const { cleanup, container, input } = createGuard();
    const focusSpy = vi.spyOn(input, 'focus');

    dispatchLongPress(container);

    expect(focusSpy).toHaveBeenCalled();
    cleanup();
  });

  it('does not focus the input on a short tap', () => {
    const { cleanup, container, input } = createGuard();
    const focusSpy = vi.spyOn(input, 'focus');

    dispatchShortTap(container);

    expect(focusSpy).not.toHaveBeenCalled();
    cleanup();
  });

  it('reveals the hidden command bar after a long press', () => {
    const { cleanup, container, scrollToCursor } = createGuard(TEST_BUFFER_MS, false);

    dispatchLongPress(container);

    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(scrollToCursor).toHaveBeenCalled();
    cleanup();
  });

  it('focuses hidden input synchronously on pointer down so mobile keyboards can open', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const focusSpy = vi.spyOn(input, 'focus');
    const pointerDown = new PointerEvent('pointerdown', {
      bubbles: true,
      cancelable: true,
      clientX: 20,
      clientY: 20,
      pointerId: 3,
      pointerType: 'touch',
    });

    const notCancelled = container.dispatchEvent(pointerDown);

    expect(notCancelled).toBe(false);
    expect(pointerDown.defaultPrevented).toBe(true);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(focusSpy).toHaveBeenCalled();
    expect(focusSpy.mock.calls[0]?.[0]).toEqual({ preventScroll: true });
    cleanup();
  });

  it('captures mouse taps in hidden mode before xterm can focus its helper textarea', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const focusSpy = vi.spyOn(input, 'focus');
    const mouseDown = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      clientX: 20,
      clientY: 20,
    });
    const mouseUp = new MouseEvent('mouseup', {
      bubbles: true,
      cancelable: true,
      clientX: 20,
      clientY: 20,
    });

    const downNotCancelled = container.dispatchEvent(mouseDown);
    const upNotCancelled = container.dispatchEvent(mouseUp);

    expect(downNotCancelled).toBe(false);
    expect(upNotCancelled).toBe(false);
    expect(mouseDown.defaultPrevented).toBe(true);
    expect(mouseUp.defaultPrevented).toBe(true);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(focusSpy).toHaveBeenCalled();
    cleanup();
  });

  it('dismisses hidden input when the terminal is tapped while input is active', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const blurSpy = vi.spyOn(input, 'blur');

    container.dispatchEvent(
      new MouseEvent('mousedown', {
        bubbles: true,
        cancelable: true,
        clientX: 20,
        clientY: 20,
      }),
    );
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);

    vi.advanceTimersByTime(400);
    const secondTap = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      clientX: 30,
      clientY: 30,
    });
    const notCancelled = container.dispatchEvent(secondTap);

    expect(notCancelled).toBe(false);
    expect(secondTap.defaultPrevented).toBe(true);
    expect(blurSpy).toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    cleanup();
  });

  it('dismisses hidden input when visual viewport expands after keyboard close', () => {
    const listeners = new Map<string, EventListener>();
    const visualViewport = {
      height: 500,
      offsetTop: 0,
      addEventListener: vi.fn((type: string, listener: EventListener) => {
        listeners.set(type, listener);
      }),
      removeEventListener: vi.fn((type: string) => {
        listeners.delete(type);
      }),
    };
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      value: 800,
    });
    Object.defineProperty(window, 'visualViewport', {
      configurable: true,
      value: visualViewport,
    });

    const { cleanup, container, input, scrollToCursor } = createGuard(TEST_BUFFER_MS, false);
    const blurSpy = vi.spyOn(input, 'blur');

    container.dispatchEvent(
      new MouseEvent('mousedown', {
        bubbles: true,
        cancelable: true,
        clientX: 20,
        clientY: 20,
      }),
    );
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(container.style.getPropertyValue('--pm-mobile-keyboard-panel-height')).toBe('300px');

    scrollToCursor.mockClear();
    visualViewport.height = 800;
    listeners.get('resize')?.(new Event('resize'));

    expect(blurSpy).toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    expect(container.style.getPropertyValue('--pm-mobile-keyboard-panel-height')).toBe('');
    expect(scrollToCursor).toHaveBeenCalled();
    cleanup();
  });

  it('captures hidden-mode long presses from terminal children before xterm handles them', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const terminalChild = document.createElement('canvas');
    container.prepend(terminalChild);
    const focusSpy = vi.spyOn(input, 'focus');
    const pointerDown = new PointerEvent('pointerdown', {
      bubbles: true,
      cancelable: true,
      clientX: 20,
      clientY: 20,
      pointerId: 2,
      pointerType: 'touch',
    });

    const notCancelled = terminalChild.dispatchEvent(pointerDown);
    vi.advanceTimersByTime(MOBILE_LONG_PRESS_MS);
    terminalChild.dispatchEvent(
      new PointerEvent('pointerup', {
        bubbles: true,
        cancelable: true,
        clientX: 20,
        clientY: 20,
        pointerId: 2,
        pointerType: 'touch',
      }),
    );

    expect(notCancelled).toBe(false);
    expect(pointerDown.defaultPrevented).toBe(true);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(focusSpy).toHaveBeenCalled();
    cleanup();
  });

  it('hides the command bar again when the input blurs in hidden mode', () => {
    const { cleanup, container, input, scrollToCursor } = createGuard(TEST_BUFFER_MS, false);

    dispatchLongPress(container);
    container.style.setProperty('--pm-mobile-keyboard-panel-height', '320px');
    scrollToCursor.mockClear();
    input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    expect(container.style.getPropertyValue('--pm-mobile-keyboard-panel-height')).toBe('');
    expect(scrollToCursor).toHaveBeenCalled();
    cleanup();
  });


  it('expands and collapses the special key palette', () => {
    const { cleanup, container } = createGuard(TEST_BUFFER_MS, false);
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;
    const grid = container.querySelector('.terminal-mobile-special-grid') as HTMLDivElement;

    expect(toggle).toBeTruthy();
    expect(grid).toBeTruthy();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(grid.hasAttribute('hidden')).toBe(true);

    toggle.click();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(grid.hasAttribute('hidden')).toBe(false);

    toggle.click();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(grid.hasAttribute('hidden')).toBe(true);
    cleanup();
  });

  it('sends special terminal keys from the expanded palette', () => {
    const { cleanup, container, emitted } = createGuard();
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;
    toggle.click();

    const up = container.querySelector('[aria-label="Up arrow"]') as HTMLButtonElement;
    const ctrlC = container.querySelector('[aria-label="Interrupt / cancel current command"]') as HTMLButtonElement;
    const escape = container.querySelector('[aria-label="Escape"]') as HTMLButtonElement;
    up.click();
    ctrlC.click();
    escape.click();

    expect(emitted).toEqual([
      { text: '\x1b[A', delivery: 'immediate' },
      { text: '\x03', delivery: 'immediate' },
      { text: '\x1b', delivery: 'immediate' },
    ]);
    cleanup();
  });

  it('flushes buffered text before sending special keys', () => {
    const { cleanup, container, emitted, input } = createGuard();
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;
    toggle.click();

    dispatchInput(input, 'hi', 'insertText');
    const tab = container.querySelector('[aria-label="Tab completion"]') as HTMLButtonElement;
    tab.click();

    expect(emitted).toEqual([
      { text: 'hi', delivery: 'immediate' },
      { text: '\t', delivery: 'immediate' },
    ]);
    cleanup();
  });

  it('shows special key controls in hidden input mode', () => {
    const { cleanup, container } = createGuard(TEST_BUFFER_MS, false);

    expect(container.querySelector('[data-mobile-terminal-arrows]')).toBeTruthy();
    expect(container.querySelector('.terminal-mobile-special-toggle')).toBeTruthy();
    cleanup();
  });

  it('flushes buffered input on dispose', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'bye', 'insertText');
    vi.advanceTimersByTime(10);
    cleanup();

    expect(emitted).toEqual([{ text: 'bye', delivery: 'immediate' }]);
  });
});
