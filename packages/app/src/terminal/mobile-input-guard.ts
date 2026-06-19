import type { Disposable } from './types';
import { isReplacementInputType, normalizeSuggestionText } from './word-replacement';

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

export type MobileInputDelivery = 'batched' | 'immediate';

export function isMobileInputDevice(): boolean {
  if (typeof navigator === 'undefined') return false;
  return (
    navigator.maxTouchPoints > 0 ||
    /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent)
  );
}

function normalizeTerminalLineEndings(text: string): string {
  return text.replace(/\r\n/g, ENTER).replace(/\n/g, ENTER);
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
  const inserted = next.slice(prefixLength);
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
  private readonly arrowControls: HTMLDivElement;
  private readonly arrowUpButton: HTMLButtonElement;
  private readonly arrowDownButton: HTMLButtonElement;
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
    this.inputZone.className = 'terminal-mobile-input-zone';
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

    this.arrowControls = document.createElement('div');
    this.arrowControls.className = 'terminal-mobile-arrow-controls';
    this.arrowControls.setAttribute('data-mobile-terminal-arrows', 'true');

    this.arrowUpButton = document.createElement('button');
    this.arrowUpButton.type = 'button';
    this.arrowUpButton.className = 'terminal-mobile-arrow-button';
    this.arrowUpButton.setAttribute('aria-label', 'Up');
    this.arrowUpButton.textContent = '↑';

    this.arrowDownButton = document.createElement('button');
    this.arrowDownButton.type = 'button';
    this.arrowDownButton.className = 'terminal-mobile-arrow-button';
    this.arrowDownButton.setAttribute('aria-label', 'Down');
    this.arrowDownButton.textContent = '↓';

    this.arrowControls.append(this.arrowUpButton, this.arrowDownButton);

    this.form.appendChild(this.input);
    this.inputZone.appendChild(this.form);

    this.container.appendChild(this.tapZone);
    this.container.appendChild(this.arrowControls);
    this.container.appendChild(this.inputZone);

    this.form.addEventListener('submit', this.onFormSubmit);
    this.input.addEventListener('input', this.onInput);
    this.input.addEventListener('keydown', this.onKeyDown);
    this.input.addEventListener('blur', this.onInputBlur);
    this.arrowUpButton.addEventListener('click', this.onArrowUpClick);
    this.arrowDownButton.addEventListener('click', this.onArrowDownClick);

    this.container.addEventListener('pointerdown', this.onContainerPointerDown, { capture: true });
    this.container.addEventListener('pointermove', this.onContainerPointerMove, { capture: true });
    this.container.addEventListener('pointerup', this.onContainerPointerUp, { capture: true });
    this.container.addEventListener('pointercancel', this.onContainerPointerUp, { capture: true });
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
    this.container.removeEventListener('click', this.onContainerClick, { capture: true });
    this.container.removeEventListener('contextmenu', this.onContainerContextMenu, { capture: true });
    this.arrowUpButton.removeEventListener('click', this.onArrowUpClick);
    this.arrowDownButton.removeEventListener('click', this.onArrowDownClick);
    this.keyboardHeightCleanup?.();
    this.keyboardHeightCleanup = null;
    this.cancelLongPress();
    this.flushToTerminal();
    if (this.tapZone.isConnected) {
      this.tapZone.remove();
    }
    if (this.arrowControls.isConnected) {
      this.arrowControls.remove();
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
        this.container.style.setProperty('--pm-mobile-keyboard-panel-height', `${obscured}px`);
        this.scheduleScrollToCursor();
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
      this.emit(payload);
    }
    this.committedText = value;
    this.notifyBufferChange();
  }

  private emit(payload: string): void {
    if (!payload) return;
    this.emitInput(payload, 'immediate');
    this.scrollToCursor();
  }

  private emitControlKey(key: keyof typeof CONTROL_KEYS): void {
    this.flushToTerminal();
    this.emit(CONTROL_KEYS[key]);
  }

  private readonly onContainerPointerDown = (event: PointerEvent): void => {
    if (!this.isLongPressTarget(event.target)) return;

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

    const dx = event.clientX - this.longPressStartX;
    const dy = event.clientY - this.longPressStartY;
    if (Math.hypot(dx, dy) > MOBILE_LONG_PRESS_MOVE_THRESHOLD_PX) {
      this.cancelLongPress();
    }
  };

  private readonly onContainerPointerUp = (event: PointerEvent): void => {
    if (this.longPressPointerId !== event.pointerId) return;

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

  private readonly onArrowUpClick = (event: MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    this.emitControlKey('ArrowUp');
  };

  private readonly onArrowDownClick = (event: MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    this.emitControlKey('ArrowDown');
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
    if (!isReplacementInputType(inputType)) return;
    const normalized = normalizeSuggestionText(this.input.value);
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
      this.emit(BACKSPACE);
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
