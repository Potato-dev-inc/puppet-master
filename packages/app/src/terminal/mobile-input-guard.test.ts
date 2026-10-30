// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_MOBILE_BUFFER_MS,
  MobileInputGuard,
  MOBILE_INPUT_ENGAGED_CLASS,
  MOBILE_INPUT_HIDDEN_CLASS,
  MOBILE_KEYBOARD_OUTSIDE_CLASS,
  MOBILE_LONG_PRESS_MS,
  SPECIAL_PANEL_CLOSE_MS,
  SPECIAL_REPEAT_DELAY_MS,
  SPECIAL_REPEAT_INTERVAL_MS,
  type MobileInputDelivery,
} from './mobile-input-guard';
import { SPECIAL_KEY_GROUPS, SPECIAL_KEYS } from './mobile-input-text';

const TEST_BUFFER_MS = 25;

/** A phone sends touchend after pointerup; the guard finishes touch presses there. */
function dispatchTouchEnd(target: HTMLElement): Event {
  const event = new Event('touchend', { bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

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

  function createGuard(bufferDelayMs = TEST_BUFFER_MS, inputVisible?: boolean, reserveKeyboardSpace?: boolean) {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const emitted: Array<{ text: string; delivery: MobileInputDelivery }> = [];
    const buffer: string[] = [];
    const scrollToCursor = vi.fn();
    const guard = new MobileInputGuard({
      container,
      bufferDelayMs,
      inputVisible,
      reserveKeyboardSpace,
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

  it('abandonGesture: a press that became a scroll does not open the keyboard on release', () => {
    const { cleanup, container, guard, input } = createGuard(TEST_BUFFER_MS, false);
    const focusSpy = vi.spyOn(input, 'focus');
    container.dispatchEvent(
      new PointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );

    guard.abandonGesture();
    container.dispatchEvent(
      new PointerEvent('pointerup', { pointerId: 1, clientX: 10, clientY: 10, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    dispatchTouchEnd(container);

    expect(focusSpy).not.toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    cleanup();
  });

  it('hidden input: a swipe (finger moved) does not open the keyboard', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const focusSpy = vi.spyOn(input, 'focus');
    container.dispatchEvent(
      new PointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    container.dispatchEvent(
      new PointerEvent('pointermove', { pointerId: 1, clientX: 10, clientY: 80, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    container.dispatchEvent(
      new PointerEvent('pointerup', { pointerId: 1, clientX: 10, clientY: 80, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    dispatchTouchEnd(container);

    expect(focusSpy).not.toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    cleanup();
  });

  it('visible input: holding past the long-press time and then swiping does not focus', () => {
    const { cleanup, container, input } = createGuard();
    const focusSpy = vi.spyOn(input, 'focus');
    container.dispatchEvent(
      new PointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    vi.advanceTimersByTime(MOBILE_LONG_PRESS_MS);
    expect(focusSpy).not.toHaveBeenCalled();
    container.dispatchEvent(
      new PointerEvent('pointermove', { pointerId: 1, clientX: 10, clientY: 80, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    container.dispatchEvent(
      new PointerEvent('pointerup', { pointerId: 1, clientX: 10, clientY: 80, bubbles: true, cancelable: true, pointerType: 'touch' }),
    );
    dispatchTouchEnd(container);

    expect(focusSpy).not.toHaveBeenCalled();
    cleanup();
  });

  it('abandonGesture keeps a focused visible command field (keyboard stays open while scrolling)', () => {
    const { cleanup, guard, input } = createGuard(TEST_BUFFER_MS, true);
    input.focus();
    expect(document.activeElement).toBe(input);

    guard.abandonGesture();

    expect(document.activeElement).toBe(input);
    cleanup();
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

  it('preserves spaces after deduped autosuggest replacements', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'much much ', 'insertReplacementText');

    expect(input.value).toBe('much ');
    expect(emitted).toEqual([{ text: 'much ', delivery: 'immediate' }]);
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

  it('flushes backspace edits immediately to the terminal', () => {
    const { cleanup, emitted, input, scrollToCursor } = createGuard();
    dispatchInput(input, 'abcd', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    expect(emitted).toEqual([{ text: 'abcd', delivery: 'immediate' }]);
    scrollToCursor.mockClear();

    dispatchInput(input, 'abc', 'deleteContentBackward');
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

  it('flushes inserted spaces immediately', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'git', 'insertText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    dispatchInput(input, 'git ', 'insertText');

    expect(emitted).toEqual([
      { text: 'git', delivery: 'immediate' },
      { text: ' ', delivery: 'immediate' },
    ]);
    cleanup();
  });

  it('preserves and flushes a deliberate space after CJK text', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, '你好', 'insertCompositionText');
    vi.advanceTimersByTime(TEST_BUFFER_MS);
    dispatchInput(input, '你好 ', 'insertText');

    expect(input.value).toBe('你好 ');
    expect(emitted).toEqual([
      { text: '你好', delivery: 'immediate' },
      { text: ' ', delivery: 'immediate' },
    ]);
    cleanup();
  });

  function mouseTap(target: HTMLElement, clientX: number, clientY: number): void {
    target.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, clientX, clientY }));
    target.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true, clientX, clientY }));
  }

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
    dispatchTouchEnd(target);
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

  it('focuses hidden input when a tap is released (inside the gesture) so mobile keyboards can open', () => {
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
    // Nothing yet while the finger is down: it may still become a scroll.
    expect(focusSpy).not.toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);

    container.dispatchEvent(
      new PointerEvent('pointerup', { bubbles: true, cancelable: true, clientX: 20, clientY: 20, pointerId: 3, pointerType: 'touch' }),
    );
    expect(focusSpy).not.toHaveBeenCalled();
    const touchEnd = dispatchTouchEnd(container);

    expect(touchEnd.defaultPrevented).toBe(true);
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

    mouseTap(container, 20, 20);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);

    vi.advanceTimersByTime(400);
    const secondTap = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      clientX: 30,
      clientY: 30,
    });
    const notCancelled = container.dispatchEvent(secondTap);
    container.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true, clientX: 30, clientY: 30 }));

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

    mouseTap(container, 20, 20);
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

  it('leaves room for the keyboard to the page when the page already ends at the keyboard', () => {
    // iPhone: 812px layout, keyboard covers 380px. The PWA shell is already 432px tall, so the
    // guard must not take another 380px off the terminal (that left it ~0px high).
    const visualViewport = { height: 432, offsetTop: 0, addEventListener: vi.fn(), removeEventListener: vi.fn() };
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 812 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: visualViewport });

    const { cleanup, container } = createGuard(TEST_BUFFER_MS, false, false);
    expect(container.classList.contains(MOBILE_KEYBOARD_OUTSIDE_CLASS)).toBe(true);
    mouseTap(container, 20, 20);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);
    expect(container.style.getPropertyValue('--pm-mobile-keyboard-panel-height')).toBe('');
    cleanup();
    expect(container.classList.contains(MOBILE_KEYBOARD_OUTSIDE_CLASS)).toBe(false);
  });

  it('notices an Android keyboard closing when it resizes the page itself (resizes-content)', () => {
    // Android with interactive-widget=resizes-content: the layout (innerHeight) shrinks with the
    // keyboard and the visual viewport always equals it, so only the drop in page height tells.
    const listeners = new Map<string, EventListener>();
    let layout = 800;
    const visualViewport = {
      get height() { return layout; },
      offsetTop: 0,
      addEventListener: vi.fn((type: string, listener: EventListener) => listeners.set(type, listener)),
      removeEventListener: vi.fn((type: string) => listeners.delete(type)),
    };
    Object.defineProperty(window, 'innerHeight', { configurable: true, get: () => layout });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: visualViewport });

    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false, false);
    const blurSpy = vi.spyOn(input, 'blur');
    mouseTap(container, 20, 20);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(true);

    layout = 450; // keyboard up
    listeners.get('resize')?.(new Event('resize'));
    layout = 800; // closed with the system's own key
    listeners.get('resize')?.(new Event('resize'));

    expect(blurSpy).toHaveBeenCalled();
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
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
    dispatchTouchEnd(terminalChild);

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
    const panel = container.querySelector('.terminal-mobile-special-panel') as HTMLDivElement;

    expect(toggle).toBeTruthy();
    expect(panel).toBeTruthy();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(panel.hasAttribute('hidden')).toBe(true);

    toggle.click();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(toggle.getAttribute('aria-label')).toBe('Hide special keys');
    expect(panel.hasAttribute('hidden')).toBe(false);

    toggle.click();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.getAttribute('aria-label')).toBe('Show special keys');
    // The panel stays mounted while it animates out, then hides.
    expect(panel.hasAttribute('hidden')).toBe(false);
    vi.advanceTimersByTime(SPECIAL_PANEL_CLOSE_MS);
    expect(panel.hasAttribute('hidden')).toBe(true);
    cleanup();
  });

  it('groups the keys into Control, Navigate and Edit sections', () => {
    const { cleanup, container } = createGuard();
    const titles = [...container.querySelectorAll('.terminal-mobile-special-title')].map((el) => el.textContent);
    const sections = [...container.querySelectorAll('.terminal-mobile-special-section')].map((section) =>
      [...section.querySelectorAll('[data-terminal-special-id]')].map((key) => key.getAttribute('data-terminal-special-id')),
    );

    expect(titles).toEqual(['Control', 'Navigate', 'Edit']);
    expect(sections).toEqual([
      ['esc', 'tab', 'backtab', 'ctrl-c', 'ctrl-d', 'ctrl-l', 'ctrl'],
      ['home', 'up', 'end', 'left', 'down', 'right'],
      ['backspace', 'forward-delete', 'return'],
    ]);
    expect(container.querySelector('[data-mobile-terminal-arrows]')).toBeTruthy();
    cleanup();
  });

  it('sends special terminal keys from the expanded palette', () => {
    const { cleanup, container, emitted } = createGuard();
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;
    toggle.click();

    const up = container.querySelector('[aria-label="Up arrow"]') as HTMLButtonElement;
    const ctrlC = container.querySelector('[aria-label="Ctrl-C"]') as HTMLButtonElement;
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

  it('sends Shift-Tab as the backtab sequence Claude Code uses to cycle modes', () => {
    const { cleanup, container, emitted } = createGuard();
    (container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement).click();

    (container.querySelector('[aria-label="Shift-Tab"]') as HTMLButtonElement).click();

    expect(emitted).toEqual([{ text: '\x1b[Z', delivery: 'immediate' }]);
    cleanup();
  });

  it('flushes buffered text before sending special keys', () => {
    const { cleanup, container, emitted, input } = createGuard();
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;
    toggle.click();

    dispatchInput(input, 'hi', 'insertText');
    const tab = container.querySelector('[aria-label="Tab"]') as HTMLButtonElement;
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

  function pointerKey(button: HTMLElement, type: 'pointerdown' | 'pointerup' | 'pointercancel' = 'pointerdown'): PointerEvent {
    const event = new PointerEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: 5,
      clientY: 5,
      pointerId: 7,
      pointerType: 'touch',
    });
    button.dispatchEvent(event);
    return event;
  }

  function openPanel(container: HTMLElement): void {
    (container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement).click();
  }

  /** Types one character the way the browser reports it: data is just the inserted character. */
  function typeChar(input: HTMLTextAreaElement, ch: string): void {
    const before = input.value;
    input.value = before + ch;
    input.setSelectionRange(input.value.length, input.value.length);
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: ch, inputType: 'insertText' }));
  }

  it('fires a key exactly once per press, ignoring the click that follows a pointer press', () => {
    const { cleanup, container, emitted } = createGuard();
    openPanel(container);
    const esc = container.querySelector('[aria-label="Escape"]') as HTMLButtonElement;

    pointerKey(esc, 'pointerdown');
    pointerKey(esc, 'pointerup');
    esc.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));

    expect(emitted).toEqual([{ text: '\x1b', delivery: 'immediate' }]);
    cleanup();
  });

  it('pressing a key does not move focus off the command field', () => {
    const { cleanup, container, input } = createGuard();
    openPanel(container);
    input.focus();
    const esc = container.querySelector('[aria-label="Escape"]') as HTMLButtonElement;
    const down = pointerKey(esc, 'pointerdown');
    const mouseDown = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    esc.dispatchEvent(mouseDown);

    expect(down.defaultPrevented).toBe(true);
    expect(mouseDown.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(input);
    cleanup();
  });

  it('pressing the toggle with a pointer does not reveal or focus the hidden input', () => {
    const { cleanup, container, input } = createGuard(TEST_BUFFER_MS, false);
    const focusSpy = vi.spyOn(input, 'focus');
    const toggle = container.querySelector('.terminal-mobile-special-toggle') as HTMLButtonElement;

    pointerKey(toggle, 'pointerdown');
    pointerKey(toggle, 'pointerup');

    expect(container.querySelector('.terminal-mobile-special-panel')?.hasAttribute('hidden')).toBe(false);
    expect(container.classList.contains(MOBILE_INPUT_ENGAGED_CLASS)).toBe(false);
    expect(focusSpy).not.toHaveBeenCalled();
    cleanup();
  });

  it('calls the haptic API on each key press when it exists', () => {
    const vibrate = vi.fn(() => true);
    Object.defineProperty(navigator, 'vibrate', { configurable: true, value: vibrate });
    try {
      const { cleanup, container } = createGuard();
      openPanel(container);
      pointerKey(container.querySelector('[aria-label="Tab"]') as HTMLElement);
      expect(vibrate).toHaveBeenCalledWith(8);
      cleanup();
    } finally {
      Reflect.deleteProperty(navigator, 'vibrate');
    }
  });

  it('auto-repeats a repeatable key while held, and stops on release', () => {
    const { cleanup, container, emitted } = createGuard();
    openPanel(container);
    const left = container.querySelector('[aria-label="Left arrow"]') as HTMLElement;

    pointerKey(left, 'pointerdown');
    expect(emitted).toHaveLength(1);

    vi.advanceTimersByTime(SPECIAL_REPEAT_DELAY_MS - 1);
    expect(emitted).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(emitted).toHaveLength(2);
    vi.advanceTimersByTime(SPECIAL_REPEAT_INTERVAL_MS);
    expect(emitted).toHaveLength(3);

    pointerKey(left, 'pointerup');
    vi.advanceTimersByTime(SPECIAL_REPEAT_INTERVAL_MS * 5);
    expect(emitted).toHaveLength(3);
    expect(emitted.every((e) => e.text === '\x1b[D')).toBe(true);
    cleanup();
  });

  it('stops auto-repeat when the pointer is cancelled', () => {
    const { cleanup, container, emitted } = createGuard();
    openPanel(container);
    const backspace = container.querySelector('[aria-label="Backspace"]') as HTMLElement;

    pointerKey(backspace, 'pointerdown');
    pointerKey(backspace, 'pointercancel');
    vi.advanceTimersByTime(SPECIAL_REPEAT_DELAY_MS * 2);

    expect(emitted).toEqual([{ text: '\x7f', delivery: 'immediate' }]);
    cleanup();
  });

  it('does not auto-repeat keys without the repeat flag', () => {
    const { cleanup, container, emitted } = createGuard();
    openPanel(container);

    pointerKey(container.querySelector('[aria-label="Escape"]') as HTMLElement);
    vi.advanceTimersByTime(SPECIAL_REPEAT_DELAY_MS * 3);

    expect(emitted).toHaveLength(1);
    cleanup();
  });

  it('Ctrl latch: the next typed letter is sent as its control code and not kept in the field', () => {
    const { cleanup, container, emitted, input } = createGuard();
    openPanel(container);
    const latch = container.querySelector('[aria-label="Control modifier"]') as HTMLButtonElement;

    pointerKey(latch);
    expect(latch.getAttribute('aria-pressed')).toBe('true');
    expect(latch.classList.contains('terminal-mobile-special-key--latched')).toBe(true);
    expect(emitted).toEqual([]);

    typeChar(input, 'c');

    expect(emitted).toEqual([{ text: '\x03', delivery: 'immediate' }]);
    expect(input.value).toBe('');
    expect(latch.getAttribute('aria-pressed')).toBe('false');
    cleanup();
  });

  it('Ctrl latch: buffered text is sent first, then the control code', () => {
    const { cleanup, container, emitted, input } = createGuard();
    openPanel(container);

    typeChar(input, 'l');
    typeChar(input, 's');
    expect(emitted).toEqual([]);
    pointerKey(container.querySelector('[aria-label="Control modifier"]') as HTMLElement);
    typeChar(input, 'd');

    expect(emitted).toEqual([
      { text: 'ls', delivery: 'immediate' },
      { text: '\x04', delivery: 'immediate' },
    ]);
    expect(input.value).toBe('ls');
    cleanup();
  });

  it('Ctrl latch: maps @[\\]^_ and ignores characters that are not letters', () => {
    const { cleanup, container, emitted, input } = createGuard();
    openPanel(container);
    const latch = container.querySelector('[aria-label="Control modifier"]') as HTMLElement;

    pointerKey(latch);
    typeChar(input, '1');
    expect(emitted).toEqual([]);
    expect(input.value).toBe('1');
    expect(latch.getAttribute('aria-pressed')).toBe('true');

    input.value = '';
    typeChar(input, '[');
    expect(emitted.at(-1)).toEqual({ text: '\x1b', delivery: 'immediate' });
    cleanup();
  });

  it('Ctrl latch: tapping the Ctrl key again unlatches it', () => {
    const { cleanup, container, emitted, input } = createGuard();
    openPanel(container);
    const latch = container.querySelector('[aria-label="Control modifier"]') as HTMLElement;

    pointerKey(latch);
    pointerKey(latch);
    typeChar(input, 'c');

    expect(emitted).toEqual([]);
    expect(input.value).toBe('c');
    cleanup();
  });

  it('closes the panel on a tap outside it, and on Escape', () => {
    const { cleanup, container } = createGuard(TEST_BUFFER_MS, false);
    const panel = container.querySelector('.terminal-mobile-special-panel') as HTMLDivElement;
    openPanel(container);

    // A tap on the keys themselves keeps it open.
    pointerKey(container.querySelector('[aria-label="Tab"]') as HTMLElement);
    expect(panel.hasAttribute('hidden')).toBe(false);

    // A tap elsewhere on the terminal closes it.
    container.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, pointerId: 9, pointerType: 'touch' }));
    vi.advanceTimersByTime(SPECIAL_PANEL_CLOSE_MS);
    expect(panel.hasAttribute('hidden')).toBe(true);

    openPanel(container);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    vi.advanceTimersByTime(SPECIAL_PANEL_CLOSE_MS);
    expect(panel.hasAttribute('hidden')).toBe(true);
    cleanup();
  });

  it('Escape with the panel open closes it and is not sent to the terminal', () => {
    const { cleanup, container, emitted, input } = createGuard();
    openPanel(container);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));

    expect(emitted).toEqual([]);
    expect(container.querySelector('.terminal-mobile-special-panel')?.classList.contains('terminal-mobile-special-panel--closing')).toBe(true);
    cleanup();
  });

  it('exposes every key through SPECIAL_KEYS, flattened from the groups', () => {
    expect(SPECIAL_KEYS).toEqual(SPECIAL_KEY_GROUPS.flatMap((group) => group.keys));
    expect(SPECIAL_KEYS.length).toBe(16);
  });

  it('flushes buffered input on dispose', () => {
    const { cleanup, emitted, input } = createGuard();

    dispatchInput(input, 'bye', 'insertText');
    vi.advanceTimersByTime(10);
    cleanup();

    expect(emitted).toEqual([{ text: 'bye', delivery: 'immediate' }]);
  });
});
