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
    const input = container.querySelector(
      '[data-mobile-terminal-input="true"]',
    ) as HTMLTextAreaElement;

    return {
      buffer,
      cleanup: () => {
        guard.dispose();
        container.remove();
      },
      container,
      emitted,
      form: container.querySelector('form.terminal-mobile-command-form') as HTMLFormElement,
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
    const { cleanup, emitted, input } = createGuard();
    dispatchInput(input, 'abcd', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted).toEqual([{ text: 'abcd', delivery: 'immediate' }]);

    dispatchInput(input, 'abc', 'deleteContentBackward');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted.at(-1)).toEqual({ text: '\x7f', delivery: 'immediate' });
    cleanup();
  });

  it('sends terminal backspace when the input field is already empty', () => {
    const { cleanup, emitted, input } = createGuard();

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true }));

    expect(emitted).toEqual([{ text: '\x7f', delivery: 'immediate' }]);
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
  ): void {
    container.dispatchEvent(
      new PointerEvent('pointerdown', {
        bubbles: true,
        clientX,
        clientY,
        pointerId: 1,
        pointerType: 'touch',
      }),
    );
    if (durationMs > 0) {
      vi.advanceTimersByTime(durationMs);
    }
    container.dispatchEvent(
      new PointerEvent('pointerup', {
        bubbles: true,
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

  it('hides the command bar again when the input blurs in hidden mode', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);

    dispatchLongPress(container);
    input.dispatchEvent(new FocusEvent('blur', { bubbles: true }));

    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    cleanup();
  });


  it('sends arrow-up to the terminal when the up button is tapped', () => {
    const { cleanup, container, emitted } = createGuard();

    const up = container.querySelector(
      '.terminal-mobile-arrow-button[aria-label="Up"]',
    ) as HTMLButtonElement;
    up.click();

    expect(emitted).toEqual([{ text: '\x1b[A', delivery: 'immediate' }]);
    cleanup();
  });

  it('sends arrow-down to the terminal when the down button is tapped', () => {
    const { cleanup, container, emitted } = createGuard();

    const down = container.querySelector(
      '.terminal-mobile-arrow-button[aria-label="Down"]',
    ) as HTMLButtonElement;
    down.click();

    expect(emitted).toEqual([{ text: '\x1b[B', delivery: 'immediate' }]);
    cleanup();
  });

  it('flushes buffered text before sending arrow keys', () => {
    const { cleanup, container, emitted, input } = createGuard();

    dispatchInput(input, 'hi', 'insertText');
    const up = container.querySelector(
      '.terminal-mobile-arrow-button[aria-label="Up"]',
    ) as HTMLButtonElement;
    up.click();

    expect(emitted).toEqual([
      { text: 'hi', delivery: 'immediate' },
      { text: '\x1b[A', delivery: 'immediate' },
    ]);
    cleanup();
  });

  it('shows arrow controls in hidden input mode', () => {
    const { cleanup, container } = createGuard(TEST_BUFFER_MS, false);

    expect(container.querySelector('[data-mobile-terminal-arrows]')).toBeTruthy();
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
