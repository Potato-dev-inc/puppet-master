import type { Disposable } from './types';
import { isReplacementInputType, normalizeSuggestionText } from './word-replacement';

const BACKSPACE = '\x7f';
const ENTER = '\r';

/** Default time to hold mobile input in memory before committing to the PTY. */
export const DEFAULT_MOBILE_BUFFER_MS = 5000;

/** CSS class on the host to hide the bar later without changing input behavior. */
export const MOBILE_INPUT_HIDDEN_CLASS = 'terminal-host--mobile-input-hidden';

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
}

/**
 * Standard HTML command field (form + text input). The browser owns typing,
 * autocorrect, and suggestions. We only forward text to the terminal after the
 * buffer timer or when the user submits the form (Enter).
 */
export class MobileInputGuard implements Disposable {
  private readonly container: HTMLElement;
  private readonly emitInput: (text: string, delivery: MobileInputDelivery) => void;
  private readonly scrollToCursor: () => void;
  private readonly onBufferChange?: (text: string) => void;
  private readonly bufferDelayMs: number;
  private readonly scrollZone: HTMLDivElement;
  private readonly inputZone: HTMLDivElement;
  private readonly form: HTMLFormElement;
  private readonly input: HTMLInputElement;
  /** Text already written to the terminal from the current field contents. */
  private committedText = '';
  private flushTimer: number | null = null;

  constructor(options: MobileInputGuardOptions) {
    this.container = options.container;
    this.emitInput = options.emitInput;
    this.scrollToCursor = options.scrollToCursor;
    this.onBufferChange = options.onBufferChange;
    this.bufferDelayMs = options.bufferDelayMs ?? DEFAULT_MOBILE_BUFFER_MS;

    if (getComputedStyle(this.container).position === 'static') {
      this.container.style.position = 'relative';
    }
    this.container.classList.add('terminal-host--mobile-input');

    this.scrollZone = document.createElement('div');
    this.scrollZone.className = 'terminal-mobile-scroll-zone';
    this.scrollZone.setAttribute('data-mobile-terminal-scroll', 'true');

    this.inputZone = document.createElement('div');
    this.inputZone.className = 'terminal-mobile-input-zone';
    this.inputZone.setAttribute('data-mobile-terminal-input-zone', 'true');

    this.form = document.createElement('form');
    this.form.className = 'terminal-mobile-command-form';
    this.form.setAttribute('autocomplete', 'off');
    this.form.noValidate = true;

    this.input = document.createElement('input');
    this.input.type = 'text';
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
    this.input.placeholder = 'Type a command…';

    this.form.appendChild(this.input);
    this.inputZone.appendChild(this.form);
    this.container.appendChild(this.scrollZone);
    this.container.appendChild(this.inputZone);

    this.form.addEventListener('submit', this.onFormSubmit);
    this.input.addEventListener('input', this.onInput);
    this.input.addEventListener('keydown', this.onKeyDown);
    this.notifyBufferChange();
  }

  dispose(): void {
    this.form.removeEventListener('submit', this.onFormSubmit);
    this.input.removeEventListener('input', this.onInput);
    this.input.removeEventListener('keydown', this.onKeyDown);
    this.flushToTerminal();
    this.scrollZone.remove();
    this.inputZone.remove();
    this.container.classList.remove('terminal-host--mobile-input');
    this.container.classList.remove(MOBILE_INPUT_HIDDEN_CLASS);
  }

  focus(): void {
    if (!this.input.isConnected) return;
    this.input.focus({ preventScroll: true });
  }

  blur(): void {
    this.input.blur();
  }

  /** Tap (not drag) on the terminal background — focus or blur the command field. */
  handleBackgroundTap(target: EventTarget | null): void {
    if (!(target instanceof Element)) return;
    if (target.closest('[data-mobile-terminal-input]')) return;
    if (target.closest('[data-mobile-terminal-input-zone]')) {
      this.focus();
      return;
    }
    this.blur();
  }

  getBufferText(): string {
    return this.input.value;
  }

  private notifyBufferChange(): void {
    this.onBufferChange?.(this.input.value);
  }

  private scheduleFlush(): void {
    this.cancelFlush();
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
