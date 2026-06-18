import type { Disposable } from './types';

const BACKSPACE = '\x7f';

const REPLACEMENT_INPUT_TYPES = new Set([
  'insertReplacementText',
  'insertFromComposition',
  'insertFromSuggestion',
  'insertCompositionText',
]);

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

/** PTY backspaces followed by the replacement text. */
export function buildReplacementInput(replacedLength: number, insertText: string): string | null {
  if (replacedLength <= 0 || insertText.length === 0) return null;
  return BACKSPACE.repeat(replacedLength) + insertText;
}

/** Longest suffix of `before` that is a prefix of `insert` (keyboard word completion). */
export function estimateReplacedPrefix(before: string, insert: string): number {
  const max = Math.min(before.length, insert.length);
  for (let length = max; length > 0; length -= 1) {
    const suffix = before.slice(-length);
    if (insert.startsWith(suffix)) return length;
  }
  return 0;
}

function normalizeTerminalLineEndings(text: string): string {
  return text.replace(/\r\n/g, '\r').replace(/\n/g, '\r');
}

/** Keyboards occasionally append a duplicated suggestion ("much much"). */
export function normalizeSuggestionText(insert: string): string {
  const trimmed = insert.replace(/\s+$/, '');
  const parts = trimmed.split(/\s+/);
  if (parts.length === 2 && parts[0] === parts[1]) {
    return parts[0];
  }
  return insert;
}

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

function currentToken(value: string): string {
  return value.match(/[^\s]*$/)?.[0] ?? '';
}

function isWordBoundaryInput(value: string): boolean {
  return /\s/.test(value);
}

function isReplacementInputType(inputType: string): boolean {
  return REPLACEMENT_INPUT_TYPES.has(inputType);
}

function shouldSendImmediately(payload: string, inputType: string): boolean {
  return (
    payload.includes(BACKSPACE) ||
    payload.includes('\r') ||
    payload.includes('\t') ||
    payload.startsWith('\x1b') ||
    isReplacementInputType(inputType)
  );
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

function configureMobileTextarea(textarea: HTMLTextAreaElement): void {
  textarea.setAttribute('data-mobile-terminal-input', 'true');
  textarea.setAttribute('aria-label', 'Terminal input');
  textarea.setAttribute('autocomplete', 'off');
  textarea.setAttribute('autocorrect', 'on');
  textarea.setAttribute('autocapitalize', 'off');
  textarea.spellcheck = true;
  textarea.inputMode = 'text';
  textarea.enterKeyHint = 'enter';
  textarea.rows = 1;
  textarea.wrap = 'off';
  textarea.style.position = 'absolute';
  textarea.style.left = '0';
  textarea.style.right = '0';
  textarea.style.bottom = '0';
  textarea.style.width = '100%';
  textarea.style.height = '44px';
  textarea.style.opacity = '0.01';
  textarea.style.color = 'transparent';
  textarea.style.caretColor = 'transparent';
  textarea.style.background = 'transparent';
  textarea.style.border = 'none';
  textarea.style.outline = 'none';
  textarea.style.resize = 'none';
  textarea.style.fontSize = '16px';
  textarea.style.lineHeight = '44px';
  textarea.style.padding = '0';
  textarea.style.margin = '0';
  textarea.style.zIndex = '30';
  textarea.style.overflow = 'hidden';
}

interface MobileInputGuardOptions {
  container: HTMLElement;
  emitInput: (text: string, delivery: MobileInputDelivery) => void;
  scrollToCursor: () => void;
}

/**
 * Mobile mirror-mode input is independent from xterm's helper textarea. The
 * phone keyboard owns this textarea natively, and we send PTY deltas after its
 * value changes so autocorrect and suggestions do not fight synthesized events.
 */
export class MobileInputGuard implements Disposable {
  private readonly container: HTMLElement;
  private readonly emitInput: (text: string, delivery: MobileInputDelivery) => void;
  private readonly scrollToCursor: () => void;
  private readonly textarea: HTMLTextAreaElement;
  private sentValue = '';
  private composing = false;
  private pendingInputType = 'insertText';

  constructor(options: MobileInputGuardOptions) {
    this.container = options.container;
    this.emitInput = options.emitInput;
    this.scrollToCursor = options.scrollToCursor;
    this.textarea = document.createElement('textarea');
    configureMobileTextarea(this.textarea);

    if (getComputedStyle(this.container).position === 'static') {
      this.container.style.position = 'relative';
    }

    this.container.appendChild(this.textarea);
    this.textarea.addEventListener('beforeinput', this.onBeforeInput);
    this.textarea.addEventListener('input', this.onInput);
    this.textarea.addEventListener('keydown', this.onKeyDown);
    this.textarea.addEventListener('compositionstart', this.onCompositionStart);
    this.textarea.addEventListener('compositionend', this.onCompositionEnd);
  }

  dispose(): void {
    this.textarea.removeEventListener('beforeinput', this.onBeforeInput);
    this.textarea.removeEventListener('input', this.onInput);
    this.textarea.removeEventListener('keydown', this.onKeyDown);
    this.textarea.removeEventListener('compositionstart', this.onCompositionStart);
    this.textarea.removeEventListener('compositionend', this.onCompositionEnd);
    this.textarea.remove();
  }

  focus(): void {
    if (!this.textarea.isConnected) return;
    this.textarea.focus({ preventScroll: true });
    this.moveCaretToEnd();
  }

  private moveCaretToEnd(): void {
    const end = this.textarea.value.length;
    this.textarea.setSelectionRange(end, end);
  }

  private setNativeValue(value: string): void {
    this.textarea.value = value;
    this.sentValue = value;
    this.moveCaretToEnd();
  }

  private resetToCurrentToken(): void {
    const token = currentToken(this.sentValue);
    if (token !== this.sentValue) {
      this.setNativeValue(token);
    }
  }

  private emit(payload: string, inputType: string): void {
    if (!payload) return;
    this.emitInput(payload, shouldSendImmediately(payload, inputType) ? 'immediate' : 'batched');
    this.scrollToCursor();
  }

  private applyNativeValue(inputType: string): void {
    const rawValue = this.textarea.value;
    const nextValue = isReplacementInputType(inputType)
      ? normalizeSuggestionText(rawValue)
      : rawValue;

    if (nextValue !== rawValue) {
      this.textarea.value = nextValue;
    }

    const payload = buildInputDelta(this.sentValue, nextValue);
    this.emit(payload, inputType);
    this.sentValue = nextValue;

    if (isWordBoundaryInput(nextValue)) {
      this.resetToCurrentToken();
    } else {
      this.moveCaretToEnd();
    }
  }

  private deleteBackward(inputType: string): void {
    if (this.sentValue.length === 0) {
      this.emit(BACKSPACE, inputType);
      return;
    }

    const deleteCount =
      inputType === 'deleteWordBackward' ? Math.max(1, currentToken(this.sentValue).length) : 1;
    const nextValue = this.sentValue.slice(0, Math.max(0, this.sentValue.length - deleteCount));
    const payload = BACKSPACE.repeat(this.sentValue.length - nextValue.length);
    this.setNativeValue(nextValue);
    this.emit(payload, inputType);
  }

  private sendControlInput(payload: string, inputType: string): void {
    this.emit(payload, inputType);
    if (payload === '\r') {
      this.setNativeValue('');
    }
  }

  private readonly onBeforeInput = (event: InputEvent): void => {
    this.pendingInputType = event.inputType || this.pendingInputType;
    if (this.composing || event.isComposing) return;

    if (event.inputType === 'deleteContentBackward' || event.inputType === 'deleteWordBackward') {
      event.preventDefault();
      event.stopImmediatePropagation();
      this.deleteBackward(event.inputType);
      return;
    }

    if (event.inputType === 'insertLineBreak' || event.inputType === 'insertParagraph') {
      event.preventDefault();
      event.stopImmediatePropagation();
      this.sendControlInput('\r', event.inputType);
    }
  };

  private readonly onInput = (event: Event): void => {
    const inputEvent = event as InputEvent;
    this.pendingInputType = inputEvent.inputType || this.pendingInputType;
    if (this.composing || inputEvent.isComposing) return;

    this.applyNativeValue(this.pendingInputType);
    inputEvent.stopImmediatePropagation();
  };

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    if (this.composing || event.isComposing) return;

    if ((event.ctrlKey || event.metaKey) && event.key.length === 1) {
      const code = event.key.toUpperCase().charCodeAt(0);
      if (code >= 65 && code <= 90) {
        event.preventDefault();
        this.sendControlInput(String.fromCharCode(code - 64), 'controlKey');
      }
      return;
    }

    if (event.key === 'Backspace') {
      event.preventDefault();
      this.deleteBackward('deleteContentBackward');
      return;
    }

    if (event.key === 'Enter') {
      event.preventDefault();
      this.sendControlInput('\r', 'insertLineBreak');
      return;
    }

    const control = CONTROL_KEYS[event.key];
    if (control) {
      event.preventDefault();
      this.sendControlInput(control, 'controlKey');
    }
  };

  private readonly onCompositionStart = (): void => {
    this.composing = true;
  };

  private readonly onCompositionEnd = (): void => {
    this.composing = false;
    this.applyNativeValue('insertFromComposition');
  };
}
