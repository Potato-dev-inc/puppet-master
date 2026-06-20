import type { Disposable } from './types';
import {
  isReplacementInputType,
  normalizeSuggestionText,
  stripCjkImeLeadingSpace,
  stripCjkImeSpaces,
} from './word-replacement';

const BACKSPACE = '\x7f';
const ENTER = '\r';

/** Default time to hold mobile input in memory before committing to the PTY. */
export const DEFAULT_MOBILE_BUFFER_MS = 250;

/** CSS class on the host to hide the bar later without changing input behavior. */
export const MOBILE_INPUT_HIDDEN_CLASS = 'terminal-host--mobile-input-hidden';

/** CSS class applied while the hidden command bar is shown after a long press. */
export const MOBILE_INPUT_ENGAGED_CLASS = 'terminal-host--mobile-input-engaged';

/** Hold duration before the command field is shown / focused. */
export const MOBILE_LONG_PRESS_MS = 450;

/** Fallback panel height before the OS keyboard reports its size. */
export const MOBILE_KEYBOARD_PANEL_FALLBACK = '42dvh';

const MOBILE_KEYBOARD_MIN_OBSCURED_PX = 100;

const MOBILE_LONG_PRESS_MOVE_THRESHOLD_PX = 12;

const CONTROL_KEYS: Record<string, string> = {
  ArrowUp: '\x1b[A',
  ArrowDown: '\x1b[B',
  ArrowRight: '\x1b[C',
  ArrowLeft: '\x1b[D',
  Delete: '\x1b[3~',
  Home: '\x1b[H',
  End: '\x1b[F',
  Escape: '\x1b',
  Tab: '\t',
};

const SPECIAL_KEYS: Array<{ label: string; value: string; title: string }> = [
  { label: 'Esc', value: CONTROL_KEYS.Escape, title: 'Escape' },
  { label: 'Ctrl-C', value: '\x03', title: 'Interrupt / cancel current command' },
  { label: 'Ctrl-D', value: '\x04', title: 'End of input / exit shell' },
  { label: 'Ctrl-Z', value: '\x1a', title: 'Suspend process' },
  { label: 'Ctrl-L', value: '\x0c', title: 'Clear screen' },
  { label: 'Tab', value: CONTROL_KEYS.Tab, title: 'Tab completion' },
  { label: 'Enter', value: ENTER, title: 'Enter' },
  { label: 'Back', value: BACKSPACE, title: 'Backspace' },
  { label: 'Del', value: CONTROL_KEYS.Delete, title: 'Delete' },
  { label: '←', value: CONTROL_KEYS.ArrowLeft, title: 'Left arrow' },
  { label: '↑', value: CONTROL_KEYS.ArrowUp, title: 'Up arrow' },
  { label: '↓', value: CONTROL_KEYS.ArrowDown, title: 'Down arrow' },
  { label: '→', value: CONTROL_KEYS.ArrowRight, title: 'Right arrow' },
  { label: 'Home', value: CONTROL_KEYS.Home, title: 'Home' },
  { label: 'End', value: CONTROL_KEYS.End, title: 'End' },
];

export type MobileInputDelivery = 'batched' | 'immediate';

export function isMobileInputDevice(): boolean {
  if (typeof navigator === 'undefined') return false;
  return (
    new URLSearchParams(window.location.search).has('pwa') ||
    navigator.maxTouchPoints > 0 ||
    /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent)
  );
}

function normalizeTerminalLineEndings(text: string): string {
  return text.replace(/\r\n/g, ENTER).replace(/\n/g, ENTER);
}

function isDeletionOnlyPayload(text: string): boolean {
  return text.length > 0 && [...text].every((ch) => ch === BACKSPACE);
}

function endsWithInsertedWhitespace(previous: string, next: string): boolean {
  if (next.length <= previous.length) return false;
  return /\s$/u.test(next.slice(previous.length));
}

/** Translate committed vs current input text into PTY keystrokes (used only on flush). */
export function buildInputDelta(previous: string, next: string): string {
  if (previous === next) return '';

  let prefixLength = 0;
  const maxPrefix = Math.min(previous.length, next.length);
  while (
    prefixLength < maxPrefix &&
    previous[prefixLength] === next[prefixLength]
  ) {
    prefixLength += 1;
  }

  const deletedLength = previous.length - prefixLength;
  const inserted = stripCjkImeLeadingSpace(
    previous.slice(0, prefixLength),
    next.slice(prefixLength),
  );
  return BACKSPACE.repeat(deletedLength) + normalizeTerminalLineEndings(inserted);
}

/** Keep xterm's internal textarea out of the mobile keyboard path. */
export function configureXtermTextareaForMobileMirror(textarea: HTMLTextAreaElement): void {
  textarea.readOnly = true;
  textarea.tabIndex = -1;
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.opacity = '0';
  textarea.style.pointerEvents = 'none';
  textarea.style.width = '1px';
  textarea.style.height = '1px';
}

interface MobileInputGuardOptions {
  container: HTMLElement;
  emitInput: (text: string, delivery: MobileInputDelivery) => void;
  scrollToCursor: () => void;
  onBufferChange?: (text: string) => void;
  /** Hold typed text in memory this long; each new keystroke resets the timer. */
  bufferDelayMs?: number;
  inputVisible?: boolean;
}

/**
 * Standard HTML command field (form + text input). The browser owns typing,
 * autocorrect, and suggestions. We only forward text to the terminal after the
 * buffer timer or when the user submits the form (Enter).
 *
 * Short taps on the terminal pass through to xterm (mouse / selection UIs).
 * Long press reveals and focuses the command field.
 */
export class MobileInputGuard implements Disposable {
  private readonly container: HTMLElement;
  private readonly emitInput: (text: string, delivery: MobileInputDelivery) => void;
  private readonly scrollToCursor: () => void;
  private readonly onBufferChange?: (text: string) => void;
  private readonly bufferDelayMs: number;
  private readonly hiddenInput: boolean;
  private readonly tapZone: HTMLDivElement;
  private readonly inputZone: HTMLDivElement;
  private readonly specialControls: HTMLDivElement;
  private readonly specialToggleButton: HTMLButtonElement;
  private readonly specialGrid: HTMLDivElement;
  private readonly form: HTMLFormElement;
  private readonly input: HTMLTextAreaElement;
  /** Text already written to the terminal from the current field contents. */
  private committedText = '';
  private flushTimer: number | null = null;
  private engaged = false;
  private longPressPointerId: number | null = null;
  private longPressStartX = 0;
  private longPressStartY = 0;
  private longPressTimer: number | null = null;
  private longPressTriggered = false;
  private suppressClick = false;
  private keyboardOpen = false;
  private hiddenInputFocusedAt = 0;
  private keyboardHeightCleanup: (() => void) | null = null;

  constructor(options: MobileInputGuardOptions) {
    this.container = options.container;
    this.emitInput = options.emitInput;
    this.scrollToCursor = options.scrollToCursor;
    this.onBufferChange = options.onBufferChange;
    this.bufferDelayMs = options.bufferDelayMs ?? DEFAULT_MOBILE_BUFFER_MS;
    this.hiddenInput = options.inputVisible === false;

    if (getComputedStyle(this.container).position === 'static') {
      this.container.style.position = 'relative';
    }
    this.container.classList.add('terminal-host--mobile-input');
    this.container.classList.toggle(MOBILE_INPUT_HIDDEN_CLASS, this.hiddenInput);

    this.tapZone = document.createElement('div');
    this.tapZone.className = 'terminal-mobile-tap-zone';
    this.tapZone.setAttribute('data-mobile-terminal-tap', 'true');

    this.inputZone = document.createElement('div');
    this.inputZone.className = this.hiddenInput
      ? 'terminal-mobile-input-zone terminal-mobile-keyboard-sink-zone'
      : 'terminal-mobile-input-zone';
    this.inputZone.setAttribute('data-mobile-terminal-input-zone', 'true');

    this.form = document.createElement('form');
    this.form.className = 'terminal-mobile-command-form';
    this.form.setAttribute('autocomplete', 'off');
    this.form.noValidate = true;

    this.input = document.createElement('textarea');
    this.input.name = 'command';
    this.input.id = `terminal-mobile-command-${Math.random().toString(36).slice(2, 9)}`;
    this.input.className = 'terminal-mobile-command-input';
    this.input.setAttribute('data-mobile-terminal-input', 'true');
    this.input.setAttribute('aria-label', 'Terminal command');
    this.input.setAttribute('autocomplete', 'on');
    this.input.setAttribute('autocorrect', 'on');
    this.input.setAttribute('autocapitalize', 'off');
    this.input.setAttribute('spellcheck', 'true');
    this.input.setAttribute('inputmode', 'text');
    this.input.setAttribute('enterkeyhint', 'send');
    this.input.rows = 1;
    this.input.wrap = 'soft';
    this.input.placeholder = 'Type a command…';

    this.specialControls = document.createElement('div');
    this.specialControls.className = 'terminal-mobile-special-controls';
    this.specialControls.setAttribute('data-mobile-terminal-arrows', 'true');

    this.specialToggleButton = document.createElement('button');
    this.specialToggleButton.type = 'button';
    this.specialToggleButton.className = 'terminal-mobile-special-toggle';
    this.specialToggleButton.setAttribute('aria-label', 'Show special keys');
    this.specialToggleButton.setAttribute('aria-expanded', 'false');
    this.specialToggleButton.textContent = 'Keys';

    this.specialGrid = document.createElement('div');
    this.specialGrid.className = 'terminal-mobile-special-grid';
    this.specialGrid.setAttribute('hidden', 'true');
    this.specialGrid.setAttribute('aria-label', 'Special terminal keys');

    for (const key of SPECIAL_KEYS) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'terminal-mobile-special-key';
      button.textContent = key.label;
      button.title = key.title;
      button.setAttribute('aria-label', key.title);
      button.dataset.terminalSpecialKey = key.value;
      this.specialGrid.appendChild(button);
    }

    this.specialControls.append(this.specialGrid, this.specialToggleButton);

    this.form.appendChild(this.input);
    this.inputZone.appendChild(this.form);

    this.container.appendChild(this.tapZone);
    this.container.appendChild(this.specialControls);
    if (this.hiddenInput) {
      document.body.appendChild(this.inputZone);
    } else {
      this.container.appendChild(this.inputZone);
    }

    this.form.addEventListener('submit', this.onFormSubmit);
    this.input.addEventListener('input', this.onInput);
    this.input.addEventListener('keydown', this.onKeyDown);
    this.input.addEventListener('blur', this.onInputBlur);
    this.specialToggleButton.addEventListener('click', this.onSpecialToggleClick);
    this.specialGrid.addEventListener('click', this.onSpecialGridClick);

    this.container.addEventListener('pointerdown', this.onContainerPointerDown, { capture: true });
    this.container.addEventListener('pointermove', this.onContainerPointerMove, { capture: true });
    this.container.addEventListener('pointerup', this.onContainerPointerUp, { capture: true });
    this.container.addEventListener('pointercancel', this.onContainerPointerUp, { capture: true });
    this.container.addEventListener('mousedown', this.onContainerMouseDown, { capture: true });
    this.container.addEventListener('mouseup', this.onContainerMouseUp, { capture: true });
    this.container.addEventListener('touchstart', this.onContainerTouchStart, { capture: true });
    this.container.addEventListener('touchend', this.onContainerTouchEnd, { capture: true });
    this.container.addEventListener('touchcancel', this.onContainerTouchEnd, { capture: true });
    this.container.addEventListener('click', this.onContainerClick, { capture: true });
    this.container.addEventListener('contextmenu', this.onContainerContextMenu, { capture: true });

    this.installKeyboardHeightSync();
    this.notifyBufferChange();
  }

  dispose(): void {
    this.form.removeEventListener('submit', this.onFormSubmit);
    this.input.removeEventListener('input', this.onInput);
    this.input.removeEventListener('keydown', this.onKeyDown);
    this.input.removeEventListener('blur', this.onInputBlur);
    this.container.removeEventListener('pointerdown', this.onContainerPointerDown, { capture: true });
    this.container.removeEventListener('pointermove', this.onContainerPointerMove, { capture: true });
    this.container.removeEventListener('pointerup', this.onContainerPointerUp, { capture: true });
    this.container.removeEventListener('pointercancel', this.onContainerPointerUp, { capture: true });
    this.container.removeEventListener('mousedown', this.onContainerMouseDown, { capture: true });
    this.container.removeEventListener('mouseup', this.onContainerMouseUp, { capture: true });
    this.container.removeEventListener('touchstart', this.onContainerTouchStart, { capture: true });
    this.container.removeEventListener('touchend', this.onContainerTouchEnd, { capture: true });
    this.container.removeEventListener('touchcancel', this.onContainerTouchEnd, { capture: true });
    this.container.removeEventListener('click', this.onContainerClick, { capture: true });
    this.container.removeEventListener('contextmenu', this.onContainerContextMenu, { capture: true });
    this.specialToggleButton.removeEventListener('click', this.onSpecialToggleClick);
    this.specialGrid.removeEventListener('click', this.onSpecialGridClick);
    this.keyboardHeightCleanup?.();
    this.keyboardHeightCleanup = null;
    this.cancelLongPress();
    this.flushToTerminal();
    if (this.tapZone.isConnected) {
      this.tapZone.remove();
    }
    if (this.specialControls.isConnected) {
      this.specialControls.remove();
    }
    this.inputZone.remove();
    this.container.classList.remove('terminal-host--mobile-input');
    this.container.classList.remove(MOBILE_INPUT_HIDDEN_CLASS);
    this.container.classList.remove(MOBILE_INPUT_ENGAGED_CLASS);
  }

  focus(): void {
    if (!this.input.isConnected) return;
    this.input.focus({ preventScroll: true });
  }

  blur(): void {
    this.input.blur();
  }

  getBufferText(): string {
    return this.input.value;
  }

  private scheduleScrollToCursor(): void {
    this.scrollToCursor();
    requestAnimationFrame(() => {
      this.scrollToCursor();
      requestAnimationFrame(() => this.scrollToCursor());
    });
  }

  private installKeyboardHeightSync(): void {
    const updatePanelHeight = (): void => {
      const vv = window.visualViewport;
      if (!vv) return;

      const obscured = Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
      if (obscured >= MOBILE_KEYBOARD_MIN_OBSCURED_PX) {
        this.keyboardOpen = true;
        this.container.style.setProperty('--pm-mobile-keyboard-panel-height', `${obscured}px`);
        this.scheduleScrollToCursor();
        return;
      }

      if (this.keyboardOpen && document.activeElement === this.input) {
        this.keyboardOpen = false;
        this.input.blur();
        this.disengageInput();
        return;
      }

      if (document.activeElement === this.input) {
        this.container.style.setProperty(
          '--pm-mobile-keyboard-panel-height',
          MOBILE_KEYBOARD_PANEL_FALLBACK,
        );
        this.scheduleScrollToCursor();
      }
    };

    const onFocus = (): void => {
      updatePanelHeight();
      this.scheduleScrollToCursor();
      window.visualViewport?.addEventListener('resize', updatePanelHeight);
      window.visualViewport?.addEventListener('scroll', updatePanelHeight);
    };

    const onBlur = (): void => {
      window.visualViewport?.removeEventListener('resize', updatePanelHeight);
      window.visualViewport?.removeEventListener('scroll', updatePanelHeight);
      this.keyboardOpen = false;
      if (this.hiddenInput && !this.engaged) {
        this.container.style.removeProperty('--pm-mobile-keyboard-panel-height');
      }
    };

    this.input.addEventListener('focus', onFocus);
    this.input.addEventListener('blur', onBlur);
    this.keyboardHeightCleanup = () => {
      this.input.removeEventListener('focus', onFocus);
      this.input.removeEventListener('blur', onBlur);
      onBlur();
    };
  }

  private engageInput(): void {
    if (!this.hiddenInput) return;
    this.engaged = true;
    this.container.classList.add(MOBILE_INPUT_ENGAGED_CLASS);
    this.scheduleScrollToCursor();
  }

  private disengageInput(): void {
    if (!this.hiddenInput || !this.engaged) return;
    this.engaged = false;
    this.container.classList.remove(MOBILE_INPUT_ENGAGED_CLASS);
    this.container.style.removeProperty('--pm-mobile-keyboard-panel-height');
    this.scheduleScrollToCursor();
  }

  private isLongPressTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    if (target.closest('[data-mobile-terminal-arrows]')) return false;
    if (target.closest('[data-mobile-terminal-input]')) return false;
    if (target.closest('[data-mobile-terminal-input-zone]')) return false;
    return true;
  }

  private cancelLongPress(): void {
    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }
    this.longPressPointerId = null;
    this.longPressTriggered = false;
  }

  private revealInputFromLongPress(): void {
    this.longPressTriggered = true;
    this.suppressClick = true;
    if (this.hiddenInput) {
      this.engageInput();
    }
    this.focus();
    this.scheduleScrollToCursor();
  }

  private notifyBufferChange(): void {
    this.onBufferChange?.(this.input.value);
  }

  private captureHiddenInputGesture(event: Event): boolean {
    if (!this.hiddenInput || !this.isLongPressTarget(event.target)) return false;
    if (this.engaged || document.activeElement === this.input) {
      if (Date.now() - this.hiddenInputFocusedAt < 350) {
        event.preventDefault();
        event.stopPropagation();
        this.suppressClick = true;
        return true;
      }
      this.input.blur();
      this.disengageInput();
      event.preventDefault();
      event.stopPropagation();
      this.suppressClick = true;
      return true;
    }
    this.engageInput();
    this.focus();
    this.hiddenInputFocusedAt = Date.now();
    this.scheduleScrollToCursor();
    event.preventDefault();
    event.stopPropagation();
    this.suppressClick = true;
    return true;
  }

  private scheduleFlush(): void {
    this.cancelFlush();
    if (this.bufferDelayMs <= 0) {
      this.flushToTerminal();
      return;
    }
    this.flushTimer = window.setTimeout(() => this.flushToTerminal(), this.bufferDelayMs);
  }

  private cancelFlush(): void {
    if (this.flushTimer !== null) {
      window.clearTimeout(this.flushTimer);
      this.flushTimer = null;
    }
  }

  private flushToTerminal(): void {
    this.cancelFlush();

    const value = this.input.value;
    const payload = buildInputDelta(this.committedText, value);
    if (payload) {
      this.emit(payload, !isDeletionOnlyPayload(payload));
    }
    this.committedText = value;
    this.notifyBufferChange();
  }

  private emit(payload: string, scroll = true): void {
    if (!payload) return;
    this.emitInput(payload, 'immediate');
    if (scroll) {
      this.scrollToCursor();
    }
  }

  private emitControlKey(key: keyof typeof CONTROL_KEYS): void {
    this.flushToTerminal();
    this.emit(CONTROL_KEYS[key]);
  }

  private emitSpecialKey(payload: string): void {
    this.flushToTerminal();
    this.emit(payload, !isDeletionOnlyPayload(payload));
  }

  private readonly onContainerPointerDown = (event: PointerEvent): void => {
    if (!this.isLongPressTarget(event.target)) return;

    if (this.captureHiddenInputGesture(event)) return;

    this.cancelLongPress();
    this.longPressPointerId = event.pointerId;
    this.longPressStartX = event.clientX;
    this.longPressStartY = event.clientY;
    this.longPressTimer = window.setTimeout(() => {
      this.longPressTimer = null;
      this.revealInputFromLongPress();
    }, MOBILE_LONG_PRESS_MS);
  };

  private readonly onContainerPointerMove = (event: PointerEvent): void => {
    if (this.longPressPointerId !== event.pointerId) return;

    if (this.hiddenInput) {
      event.preventDefault();
      event.stopPropagation();
    }

    const dx = event.clientX - this.longPressStartX;
    const dy = event.clientY - this.longPressStartY;
    if (Math.hypot(dx, dy) > MOBILE_LONG_PRESS_MOVE_THRESHOLD_PX) {
      this.cancelLongPress();
    }
  };

  private readonly onContainerPointerUp = (event: PointerEvent): void => {
    if (this.longPressPointerId !== event.pointerId) return;

    if (this.hiddenInput) {
      event.preventDefault();
      event.stopPropagation();
    }

    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }

    if (this.longPressTriggered) {
      event.preventDefault();
      event.stopPropagation();
      this.suppressClick = true;
    }

    this.longPressPointerId = null;
    this.longPressTriggered = false;
  };

  private readonly onContainerMouseDown = (event: MouseEvent): void => {
    this.captureHiddenInputGesture(event);
  };

  private readonly onContainerMouseUp = (event: MouseEvent): void => {
    if (!this.hiddenInput || !this.engaged) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onContainerTouchStart = (event: TouchEvent): void => {
    this.captureHiddenInputGesture(event);
  };

  private readonly onContainerTouchEnd = (event: TouchEvent): void => {
    if (!this.hiddenInput || !this.engaged) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onContainerClick = (event: MouseEvent): void => {
    if (!this.suppressClick) return;
    event.preventDefault();
    event.stopPropagation();
    this.suppressClick = false;
  };

  private readonly onContainerContextMenu = (event: MouseEvent): void => {
    if (!this.longPressTriggered && this.longPressTimer === null) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onInputBlur = (): void => {
    this.disengageInput();
  };

  private readonly onSpecialToggleClick = (event: MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    const nextOpen = this.specialGrid.hasAttribute('hidden');
    this.specialGrid.toggleAttribute('hidden', !nextOpen);
    this.specialControls.classList.toggle('terminal-mobile-special-controls--open', nextOpen);
    this.specialToggleButton.setAttribute('aria-expanded', String(nextOpen));
    this.specialToggleButton.setAttribute(
      'aria-label',
      nextOpen ? 'Hide special keys' : 'Show special keys',
    );
  };

  private readonly onSpecialGridClick = (event: MouseEvent): void => {
    const target = event.target instanceof HTMLElement
      ? event.target.closest<HTMLButtonElement>('[data-terminal-special-key]')
      : null;
    if (!target?.dataset.terminalSpecialKey) return;
    event.preventDefault();
    event.stopPropagation();
    this.emitSpecialKey(target.dataset.terminalSpecialKey);
  };

  private commitEnter(): void {
    this.flushToTerminal();
    this.emit(ENTER);
    this.input.value = '';
    this.committedText = '';
    this.cancelFlush();
    this.notifyBufferChange();
  }

  private normalizeFieldValue(inputType: string): void {
    let normalized = this.input.value;
    if (isReplacementInputType(inputType)) {
      normalized = normalizeSuggestionText(normalized);
    }
    normalized = stripCjkImeSpaces(normalized);
    if (normalized !== this.input.value) {
      this.input.value = normalized;
    }
  }

  private readonly onFormSubmit = (event: SubmitEvent): void => {
    event.preventDefault();
    this.commitEnter();
  };

  private readonly onInput = (event: Event): void => {
    const inputEvent = event as InputEvent;
    if (inputEvent.isComposing) return;

    this.normalizeFieldValue(inputEvent.inputType || 'insertText');
    this.notifyBufferChange();
    const inputType = inputEvent.inputType || 'insertText';
    if (inputType === 'deleteContentBackward' || inputType === 'deleteContentForward') {
      this.flushToTerminal();
      return;
    }
    if (endsWithInsertedWhitespace(this.committedText, this.input.value)) {
      this.flushToTerminal();
      return;
    }
    this.scheduleFlush();
  };

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    if (event.isComposing) return;

    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      this.commitEnter();
      return;
    }

    if (event.key === 'Backspace' && this.input.value.length === 0 && this.committedText.length === 0) {
      event.preventDefault();
      this.emit(BACKSPACE, false);
      return;
    }

    if ((event.ctrlKey || event.metaKey) && event.key.length === 1) {
      const code = event.key.toUpperCase().charCodeAt(0);
      if (code >= 65 && code <= 90) {
        event.preventDefault();
        this.flushToTerminal();
        this.emit(String.fromCharCode(code - 64));
      }
      return;
    }

    const control = CONTROL_KEYS[event.key];
    if (control && event.key !== 'Enter') {
      event.preventDefault();
      this.flushToTerminal();
      this.emit(control);
    }
  };
}
