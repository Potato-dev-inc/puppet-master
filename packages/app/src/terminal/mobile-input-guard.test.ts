import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  buildInputDelta,
  DEFAULT_MOBILE_BUFFER_MS,
  MobileInputGuard,
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
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function createGuard(bufferDelayMs = TEST_BUFFER_MS) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const emitted: Array<{ text: string; delivery: MobileInputDelivery }> = [];
    const buffer: string[] = [];
    const scrollToCursor = vi.fn();
    const guard = new MobileInputGuard({
      container,
      bufferDelayMs,
      emitInput: (text, delivery) => emitted.push({ text, delivery }),
      scrollToCursor,
      onBufferChange: (text) => buffer.push(text),
    });
    const input = container.querySelector(
      'input[data-mobile-terminal-input="true"]',
    ) as HTMLInputElement;

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

  function dispatchInput(input: HTMLInputElement, value: string, inputType: string): void {
    input.value = value;
    input.dispatchEvent(
      new InputEvent('input', {
        bubbles: true,
        data: value,
        inputType,
      }),
    );
  }

  it('defaults to a five second memory buffer', () => {
    expect(DEFAULT_MOBILE_BUFFER_MS).toBe(5000);
  });

  it('uses a normal HTML text input inside a form', () => {
    const { cleanup, form, input } = createGuard();

    expect(form).toBeTruthy();
    expect(input.type).toBe('text');
    expect(input.getAttribute('autocorrect')).toBe('on');
    cleanup();
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

  it('focuses the input when the bottom-half tap zone is tapped', () => {
    const { cleanup, container, guard, input } = createGuard();
    const focusSpy = vi.spyOn(input, 'focus');
    const zone = container.querySelector('[data-mobile-terminal-input-zone]') as HTMLDivElement;

    guard.handleBackgroundTap(zone);

    expect(focusSpy).toHaveBeenCalled();
    cleanup();
  });

  it('blurs the input when the top-half scroll zone is tapped', () => {
    const { cleanup, container, guard, input } = createGuard();
    guard.focus();
    const blurSpy = vi.spyOn(input, 'blur');
    const zone = container.querySelector('[data-mobile-terminal-scroll]') as HTMLDivElement;

    guard.handleBackgroundTap(zone);

    expect(blurSpy).toHaveBeenCalled();
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
