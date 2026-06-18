import { describe, expect, it, vi } from 'vitest';
import {
  buildInputDelta,
  buildReplacementInput,
  estimateReplacedPrefix,
  MobileInputGuard,
  type MobileInputDelivery,
} from './mobile-input-guard';

describe('buildReplacementInput', () => {
  it('prepends backspaces for replaced characters', () => {
    expect(buildReplacementInput(3, 'could')).toBe('\x7f\x7f\x7f' + 'could');
  });

  it('returns null when nothing is replaced', () => {
    expect(buildReplacementInput(0, 'could')).toBeNull();
    expect(buildReplacementInput(2, '')).toBeNull();
  });
});

describe('estimateReplacedPrefix', () => {
  it('detects overlapping word completion', () => {
    expect(estimateReplacedPrefix('cou', 'could')).toBe(3);
    expect(estimateReplacedPrefix('co', 'could')).toBe(2);
  });

  it('returns zero when insert does not continue the typed prefix', () => {
    expect(estimateReplacedPrefix('abc', 'hello')).toBe(0);
  });
});

describe('buildInputDelta', () => {
  it('appends ordinary typing', () => {
    expect(buildInputDelta('suf', 'sufh')).toBe('h');
  });

  it('replaces unrelated autocorrect text', () => {
    expect(buildInputDelta('sufh', 'much')).toBe('\x7f\x7f\x7f\x7fmuch');
  });

  it('only sends the completed suffix for prefix completions', () => {
    expect(buildInputDelta('cou', 'could')).toBe('ld');
  });

  it('normalizes pasted line endings for terminal input', () => {
    expect(buildInputDelta('', 'echo hi\n')).toBe('echo hi\r');
  });
});

describe('MobileInputGuard', () => {
  function createGuard() {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const emitted: Array<{ text: string; delivery: MobileInputDelivery }> = [];
    const scrollToCursor = vi.fn();
    const guard = new MobileInputGuard({
      container,
      emitInput: (text, delivery) => emitted.push({ text, delivery }),
      scrollToCursor,
    });
    const textarea = container.querySelector(
      'textarea[data-mobile-terminal-input="true"]',
    ) as HTMLTextAreaElement;

    return {
      container,
      emitted,
      guard,
      scrollToCursor,
      textarea,
      cleanup: () => {
        guard.dispose();
        container.remove();
      },
    };
  }

  function dispatchInput(textarea: HTMLTextAreaElement, value: string, inputType: string): void {
    textarea.value = value;
    textarea.dispatchEvent(
      new InputEvent('input', {
        bubbles: true,
        data: value,
        inputType,
      }),
    );
  }

  it('sends ordinary mobile text as batched PTY input', () => {
    const { cleanup, emitted, textarea } = createGuard();

    dispatchInput(textarea, 's', 'insertText');
    dispatchInput(textarea, 'su', 'insertText');

    expect(emitted).toEqual([
      { text: 's', delivery: 'batched' },
      { text: 'u', delivery: 'batched' },
    ]);
    expect(textarea.value).toBe('su');
    cleanup();
  });

  it('converts native word replacement into PTY backspaces plus the chosen word', () => {
    const { cleanup, emitted, textarea } = createGuard();

    dispatchInput(textarea, 'sufh', 'insertText');
    dispatchInput(textarea, 'much', 'insertReplacementText');

    expect(emitted).toEqual([
      { text: 'sufh', delivery: 'batched' },
      { text: '\x7f\x7f\x7f\x7fmuch', delivery: 'immediate' },
    ]);
    expect(textarea.value).toBe('much');
    cleanup();
  });

  it('dedupes replacement recovery text from mobile keyboards', () => {
    const { cleanup, emitted, textarea } = createGuard();

    dispatchInput(textarea, 'sufh', 'insertText');
    dispatchInput(textarea, 'much much', 'insertReplacementText');

    expect(emitted.at(-1)).toEqual({
      text: '\x7f\x7f\x7f\x7fmuch',
      delivery: 'immediate',
    });
    expect(textarea.value).toBe('much');
    cleanup();
  });

  it('turns deleteContentBackward into a PTY backspace and updates the native word', () => {
    const { cleanup, emitted, textarea } = createGuard();
    dispatchInput(textarea, 'abcd', 'insertText');

    const event = new InputEvent('beforeinput', {
      bubbles: true,
      cancelable: true,
      inputType: 'deleteContentBackward',
    });
    const dispatched = textarea.dispatchEvent(event);

    expect(dispatched).toBe(false);
    expect(event.defaultPrevented).toBe(true);
    expect(emitted.at(-1)).toEqual({ text: '\x7f', delivery: 'immediate' });
    expect(textarea.value).toBe('abc');
    cleanup();
  });

  it('still sends backspace when the mobile word field is empty', () => {
    const { cleanup, emitted, textarea } = createGuard();

    textarea.dispatchEvent(
      new InputEvent('beforeinput', {
        bubbles: true,
        cancelable: true,
        inputType: 'deleteContentBackward',
      }),
    );

    expect(emitted).toEqual([{ text: '\x7f', delivery: 'immediate' }]);
    expect(textarea.value).toBe('');
    cleanup();
  });

  it('keeps only the current token after whitespace so suggestions stay word-scoped', () => {
    const { cleanup, emitted, textarea } = createGuard();

    dispatchInput(textarea, 'git ', 'insertText');
    dispatchInput(textarea, 's', 'insertText');

    expect(emitted).toEqual([
      { text: 'git ', delivery: 'batched' },
      { text: 's', delivery: 'batched' },
    ]);
    expect(textarea.value).toBe('s');
    cleanup();
  });

  it('sends Enter as carriage return and clears the mobile word field', () => {
    const { cleanup, emitted, textarea } = createGuard();
    dispatchInput(textarea, 'ls', 'insertText');

    textarea.dispatchEvent(
      new InputEvent('beforeinput', {
        bubbles: true,
        cancelable: true,
        inputType: 'insertLineBreak',
      }),
    );

    expect(emitted.at(-1)).toEqual({ text: '\r', delivery: 'immediate' });
    expect(textarea.value).toBe('');
    cleanup();
  });

  it('waits for compositionend before sending IME text', () => {
    const { cleanup, emitted, textarea } = createGuard();

    textarea.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    dispatchInput(textarea, '候', 'insertCompositionText');
    expect(emitted).toEqual([]);

    textarea.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    expect(emitted).toEqual([{ text: '候', delivery: 'immediate' }]);
    cleanup();
  });
});
