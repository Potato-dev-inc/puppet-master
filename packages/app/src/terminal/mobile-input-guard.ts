// MobileInputGuard: keeps the phone keyboard away from xterm. A hidden command box buffers typing
// (IME/autocorrect-safe), diffs it into PTY input on flush, and exposes a special-key row and long-press engage.
import type { Disposable } from './types';
import {
  BACKSPACE,
  buildInputDelta,
  CONTROL_KEYS,
  controlCodeFor,
  endsWithInsertedWhitespace,
  ENTER,
  isDeletionOnlyPayload,
  SPECIAL_KEY_GROUPS,
  type SpecialKey,
} from './mobile-input-text';
import {
  isReplacementInputType,
  normalizeSuggestionText,
  stripCjkImeSpaces,
} from './word-replacement';

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

/** Special keys with `repeat`: delay before auto-repeat starts, then the interval while held. */
export const SPECIAL_REPEAT_DELAY_MS = 400;
export const SPECIAL_REPEAT_INTERVAL_MS = 60;

/** Length of the special-keys panel close animation (matches the CSS). */
export const SPECIAL_PANEL_CLOSE_MS = 220;

const SPECIAL_HAPTIC_MS = 8;

const KEYBOARD_ICON_SVG =
  '<svg class="terminal-mobile-special-icon terminal-mobile-special-icon--keyboard" viewBox="0 0 24 24" aria-hidden="true" focusable="false">' +
  '<rect x="2" y="5.5" width="20" height="13" rx="3"/>' +
  '<path d="M6 9.5h.01M10 9.5h.01M14 9.5h.01M18 9.5h.01M6 13h.01M18 13h.01M9.5 15.5h5"/>' +
  '</svg>';

const CLOSE_ICON_SVG =
  '<svg class="terminal-mobile-special-icon terminal-mobile-special-icon--close" viewBox="0 0 24 24" aria-hidden="true" focusable="false">' +
  '<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>' +
  '</svg>';

export type MobileInputDelivery = 'batched' | 'immediate';

export function isMobileInputDevice(): boolean {
  if (typeof navigator === 'undefined') return false;
  return (
    new URLSearchParams(window.location.search).has('pwa') ||
    navigator.maxTouchPoints > 0 ||
    /Android|iPhone|iPad|iPod|Mobile/i.test(navigator.userAgent)
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

interface MobileInputGuardOptions {
  container: HTMLElement;
  emitInput: (text: string, delivery: MobileInputDelivery) => void;
  scrollToCursor: () => void;
  onBufferChange?: (text: string) => void;
  /** Hold typed text in memory this long; each new keystroke resets the timer. */
  bufferDelayMs?: number;
  inputVisible?: boolean;
  /**
   * False when the page itself already ends at the software keyboard (mobile agent tabs: the
   * shell follows the visual viewport). The terminal is then not shortened by the keyboard
   * height again, and a visible input is a plain bar instead of a keyboard-tall panel.
   */
  reserveKeyboardSpace?: boolean;
}

/** Container class when the page, not this guard, makes room for the keyboard. */
export const MOBILE_KEYBOARD_OUTSIDE_CLASS = 'terminal-host--keyboard-outside';

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
  private readonly specialPanel: HTMLDivElement;
  /** Key definition for each key button on the panel. */
  private readonly specialButtons = new Map<HTMLButtonElement, SpecialKey>();
  private ctrlLatchButton: HTMLButtonElement | null = null;
  /** Ctrl latch: the next typed letter is sent as its control code. */
  private ctrlLatched = false;
  private specialPanelOpen = false;
  private specialRepeatTimer: number | null = null;
  private specialPanelCloseTimer: number | null = null;
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
  /** Hidden input: a touch is down that will toggle the keyboard on release unless it moves. */
  private tapPending = false;
  private tapStartX = 0;
  private tapStartY = 0;
  /** Visible input: the press was held long enough; the field is revealed on release. */
  private longPressReady = false;
  private keyboardHeightCleanup: (() => void) | null = null;
  /** Tallest layout height seen at the current width: the page without a keyboard. */
  private layoutBaseline = { width: 0, height: 0 };
  private readonly reserveKeyboardSpace: boolean;

  constructor(options: MobileInputGuardOptions) {
    this.container = options.container;
    this.emitInput = options.emitInput;
    this.scrollToCursor = options.scrollToCursor;
    this.onBufferChange = options.onBufferChange;
    this.bufferDelayMs = options.bufferDelayMs ?? DEFAULT_MOBILE_BUFFER_MS;
    this.hiddenInput = options.inputVisible === false;
    this.reserveKeyboardSpace = options.reserveKeyboardSpace ?? true;
    this.layoutBaseline = { width: window.innerWidth, height: window.innerHeight };

    if (getComputedStyle(this.container).position === 'static') {
      this.container.style.position = 'relative';
    }
    this.container.classList.add('terminal-host--mobile-input');
    this.container.classList.toggle(MOBILE_INPUT_HIDDEN_CLASS, this.hiddenInput);
    this.container.classList.toggle(MOBILE_KEYBOARD_OUTSIDE_CLASS, !this.reserveKeyboardSpace);

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

    this.specialPanel = document.createElement('div');
    this.specialPanel.className = 'terminal-mobile-special-panel';
    this.specialPanel.setAttribute('hidden', 'true');
    this.specialPanel.setAttribute('role', 'group');
    this.specialPanel.setAttribute('aria-label', 'Special terminal keys');

    for (const group of SPECIAL_KEY_GROUPS) {
      const section = document.createElement('div');
      section.className = `terminal-mobile-special-section terminal-mobile-special-section--${group.id}`;
      const title = document.createElement('div');
      title.className = 'terminal-mobile-special-title';
      title.textContent = group.title;
      const grid = document.createElement('div');
      grid.className = 'terminal-mobile-special-grid';
      for (const key of group.keys) {
        grid.appendChild(this.createSpecialButton(key));
      }
      section.append(title, grid);
      this.specialPanel.appendChild(section);
    }

    this.specialToggleButton = document.createElement('button');
    this.specialToggleButton.type = 'button';
    this.specialToggleButton.className = 'terminal-mobile-special-toggle';
    this.specialToggleButton.setAttribute('aria-label', 'Show special keys');
    this.specialToggleButton.setAttribute('aria-expanded', 'false');
    // Keyboard glyph and close glyph are cross-faded by CSS when the panel opens.
    this.specialToggleButton.innerHTML = KEYBOARD_ICON_SVG + CLOSE_ICON_SVG;

    this.specialControls.append(this.specialPanel, this.specialToggleButton);

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
    this.specialToggleButton.addEventListener('pointerdown', this.onSpecialTogglePointerDown);
    this.specialToggleButton.addEventListener('click', this.onSpecialToggleClick);
    this.specialControls.addEventListener('pointerdown', this.onSpecialPointerDown);
    this.specialControls.addEventListener('pointerup', this.onSpecialPointerEnd);
    this.specialControls.addEventListener('pointercancel', this.onSpecialPointerEnd);
    this.specialControls.addEventListener('mousedown', this.onSpecialMouseDown);
    this.specialControls.addEventListener('click', this.onSpecialClick);

    this.container.addEventListener('pointerdown', this.onContainerPointerDown, { capture: true });
    this.container.addEventListener('pointermove', this.onContainerPointerMove, { capture: true });
    this.container.addEventListener('pointerup', this.onContainerPointerUp, { capture: true });
    this.container.addEventListener('pointercancel', this.onContainerPointerUp, { capture: true });
    this.container.addEventListener('mousedown', this.onContainerMouseDown, { capture: true });
    this.container.addEventListener('mouseup', this.onContainerMouseUp, { capture: true });
    this.container.addEventListener('touchstart', this.onContainerTouchStart, { capture: true });
    this.container.addEventListener('touchend', this.onContainerTouchEnd, { capture: true });
    this.container.addEventListener('touchmove', this.onContainerTouchMove, { capture: true });
    this.container.addEventListener('touchcancel', this.onContainerTouchCancel, { capture: true });
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
    this.container.removeEventListener('touchmove', this.onContainerTouchMove, { capture: true });
    this.container.removeEventListener('touchcancel', this.onContainerTouchCancel, { capture: true });
    this.container.removeEventListener('click', this.onContainerClick, { capture: true });
    this.container.removeEventListener('contextmenu', this.onContainerContextMenu, { capture: true });
    this.specialToggleButton.removeEventListener('pointerdown', this.onSpecialTogglePointerDown);
    this.specialToggleButton.removeEventListener('click', this.onSpecialToggleClick);
    this.specialControls.removeEventListener('pointerdown', this.onSpecialPointerDown);
    this.specialControls.removeEventListener('pointerup', this.onSpecialPointerEnd);
    this.specialControls.removeEventListener('pointercancel', this.onSpecialPointerEnd);
    this.specialControls.removeEventListener('mousedown', this.onSpecialMouseDown);
    this.specialControls.removeEventListener('click', this.onSpecialClick);
    this.stopSpecialRepeat();
    this.closeSpecialPanel();
    this.cancelSpecialPanelClose();
    this.setCtrlLatched(false);
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
    this.container.classList.remove(MOBILE_KEYBOARD_OUTSIDE_CLASS);
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

  /**
   * The touch turned into a scroll: the keyboard is not toggled when the finger lifts. An open
   * keyboard stays open (the user may scroll up mid-typing).
   */
  abandonGesture(): void {
    this.cancelLongPress();
    this.tapPending = false;
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

      const obscured = this.keyboardObscuredPx(vv);
      if (obscured >= MOBILE_KEYBOARD_MIN_OBSCURED_PX) {
        this.keyboardOpen = true;
        if (this.reserveKeyboardSpace) {
          this.container.style.setProperty('--pm-mobile-keyboard-panel-height', `${obscured}px`);
        }
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
        if (this.reserveKeyboardSpace) {
          this.container.style.setProperty(
            '--pm-mobile-keyboard-panel-height',
            MOBILE_KEYBOARD_PANEL_FALLBACK,
          );
        }
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

  /**
   * How much of the page the software keyboard covers. iOS (and Android by default) shrink only
   * the visual viewport; with `interactive-widget=resizes-content` (mobile agent tabs) Android
   * shrinks the layout itself, which is seen as the page getting shorter than its usual height.
   */
  private keyboardObscuredPx(vv: VisualViewport): number {
    const { innerWidth, innerHeight } = window;
    if (innerWidth !== this.layoutBaseline.width) this.layoutBaseline = { width: innerWidth, height: innerHeight };
    this.layoutBaseline.height = Math.max(this.layoutBaseline.height, innerHeight);
    return Math.max(0, innerHeight - vv.height - vv.offsetTop, this.layoutBaseline.height - innerHeight);
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
    if (target.closest('[data-terminal-scroll-bottom]')) return false;
    return true;
  }

  private cancelLongPress(): void {
    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }
    this.longPressPointerId = null;
    this.longPressTriggered = false;
    this.longPressReady = false;
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

  /**
   * Hidden input, finger (or mouse) down: nothing happens yet. The keyboard is toggled only when a
   * press is released without moving (`finishHiddenTap`), so a swipe scrolls instead of popping the
   * keyboard. Called for pointerdown, touchstart and mousedown of the same press (deduped).
   */
  private captureHiddenInputGesture(event: Event): boolean {
    if (!this.hiddenInput || !this.isLongPressTarget(event.target)) return false;
    event.preventDefault();
    event.stopPropagation();
    this.suppressClick = true;
    if (!this.tapPending) {
      this.tapPending = true;
      const point = eventPoint(event);
      this.tapStartX = point?.x ?? 0;
      this.tapStartY = point?.y ?? 0;
    }
    return true;
  }

  private trackTapMove(event: Event): void {
    if (!this.tapPending) return;
    const point = eventPoint(event);
    if (!point) return;
    if (Math.hypot(point.x - this.tapStartX, point.y - this.tapStartY) > MOBILE_LONG_PRESS_MOVE_THRESHOLD_PX) {
      this.tapPending = false;
    }
  }

  /** Released: a hidden-input press that did not move toggles the keyboard now (inside the gesture). */
  private finishHiddenTap(event: Event): boolean {
    if (!this.tapPending) return false;
    this.tapPending = false;
    const point = eventPoint(event);
    if (point && Math.hypot(point.x - this.tapStartX, point.y - this.tapStartY) > MOBILE_LONG_PRESS_MOVE_THRESHOLD_PX) {
      return false;
    }
    event.preventDefault();
    event.stopPropagation();
    this.suppressClick = true;
    this.toggleHiddenInput();
    return true;
  }

  private toggleHiddenInput(): void {
    if (this.engaged || document.activeElement === this.input) {
      // Focused but no keyboard on screen (closed with the system's own key, or never shown):
      // this tap means "type", so focus again inside the gesture, which shows the keyboard.
      const vv = window.visualViewport;
      if (vv && this.keyboardObscuredPx(vv) < MOBILE_KEYBOARD_MIN_OBSCURED_PX) {
        this.input.blur();
        this.engageInput();
        this.focus();
        this.scheduleScrollToCursor();
        return;
      }
      this.input.blur();
      this.disengageInput();
      return;
    }
    this.engageInput();
    this.focus();
    this.scheduleScrollToCursor();
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
      // Held long enough: reveal on release, so holding and then swiping still scrolls.
      this.longPressReady = true;
    }, MOBILE_LONG_PRESS_MS);
  };

  private readonly onContainerPointerMove = (event: PointerEvent): void => {
    this.trackTapMove(event);
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
    // A touch press is finished on touchend instead: iOS shows the keyboard for a focus() made
    // there, and pointerup comes first.
    const touch = event.pointerType === 'touch';
    if (!touch) this.finishHiddenTap(event);
    if (this.longPressPointerId !== event.pointerId) return;

    if (this.hiddenInput) {
      event.preventDefault();
      event.stopPropagation();
    }

    if (this.longPressTimer !== null) {
      window.clearTimeout(this.longPressTimer);
      this.longPressTimer = null;
    }

    if (this.longPressReady && !touch) {
      this.longPressReady = false;
      this.revealInputFromLongPress();
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
    if (this.finishHiddenTap(event)) return;
    if (!this.hiddenInput || !this.engaged) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onContainerTouchStart = (event: TouchEvent): void => {
    this.captureHiddenInputGesture(event);
  };

  private readonly onContainerTouchMove = (event: TouchEvent): void => {
    this.trackTapMove(event);
  };

  private readonly onContainerTouchEnd = (event: TouchEvent): void => {
    if (this.longPressReady) {
      this.longPressReady = false;
      event.preventDefault();
      event.stopPropagation();
      this.revealInputFromLongPress();
      this.longPressTriggered = false;
      return;
    }
    if (this.finishHiddenTap(event)) return;
    if (!this.hiddenInput || !this.engaged) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onContainerTouchCancel = (): void => {
    this.tapPending = false;
    this.longPressReady = false;
  };

  private readonly onContainerClick = (event: MouseEvent): void => {
    if (!this.suppressClick) return;
    // The keys panel handles its own clicks; a gesture that just ended must not swallow them.
    if (event.target instanceof Element && event.target.closest('[data-mobile-terminal-arrows]')) return;
    event.preventDefault();
    event.stopPropagation();
    this.suppressClick = false;
  };

  private readonly onContainerContextMenu = (event: MouseEvent): void => {
    if (!this.longPressTriggered && !this.longPressReady && this.longPressTimer === null) return;
    event.preventDefault();
    event.stopPropagation();
  };

  private readonly onInputBlur = (): void => {
    this.disengageInput();
  };

  /**
   * Keys and the toggle act on pointerdown (fast, and it does not move focus: the phone keyboard
   * stays up). The click that follows a pointer press is ignored; only keyboard activation
   * (detail 0) goes through the click path.
   */
  private readonly onSpecialTogglePointerDown = (event: PointerEvent): void => {
    event.preventDefault();
    if (event.button > 0) return;
    this.toggleSpecialPanel();
  };

  private readonly onSpecialToggleClick = (event: MouseEvent): void => {
    event.preventDefault();
    event.stopPropagation();
    if (event.detail === 0) this.toggleSpecialPanel();
  };

  private readonly onSpecialMouseDown = (event: MouseEvent): void => {
    // Pressing the panel must not move focus off the command field (that would drop the keyboard).
    event.preventDefault();
  };

  private readonly onSpecialPointerDown = (event: PointerEvent): void => {
    const button = this.specialButtonFrom(event.target);
    if (!button) return;
    event.preventDefault();
    if (event.button > 0) return;
    this.pressSpecialKey(button, true);
  };

  private readonly onSpecialPointerEnd = (): void => {
    this.stopSpecialRepeat();
  };

  private readonly onSpecialClick = (event: MouseEvent): void => {
    const button = this.specialButtonFrom(event.target);
    if (!button || event.detail > 0) return;
    event.preventDefault();
    this.pressSpecialKey(button, false);
  };

  private specialButtonFrom(target: EventTarget | null): HTMLButtonElement | null {
    if (!(target instanceof Element)) return null;
    const button = target.closest<HTMLButtonElement>('[data-terminal-special-id]');
    return button && this.specialButtons.has(button) ? button : null;
  }

  private createSpecialButton(key: SpecialKey): HTMLButtonElement {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'terminal-mobile-special-key';
    button.dataset.terminalSpecialId = key.id;
    if (key.value) button.dataset.terminalSpecialKey = key.value;
    if (key.wide) button.classList.add('terminal-mobile-special-key--wide');
    button.setAttribute('aria-label', key.label);
    button.title = key.label;
    if (key.latch) {
      button.setAttribute('aria-pressed', 'false');
      this.ctrlLatchButton = button;
    }

    const glyph = document.createElement('span');
    glyph.className = 'terminal-mobile-special-glyph';
    glyph.setAttribute('aria-hidden', 'true');
    glyph.textContent = key.glyph;
    button.appendChild(glyph);

    if (key.caption) {
      const caption = document.createElement('span');
      caption.className = 'terminal-mobile-special-caption';
      caption.setAttribute('aria-hidden', 'true');
      caption.textContent = key.caption;
      button.appendChild(caption);
    }

    button.addEventListener('pointerleave', this.onSpecialPointerEnd);
    this.specialButtons.set(button, key);
    return button;
  }

  private pressSpecialKey(button: HTMLButtonElement, pointer: boolean): void {
    const key = this.specialButtons.get(button);
    if (!key) return;
    if (typeof navigator !== 'undefined') navigator.vibrate?.(SPECIAL_HAPTIC_MS);
    this.activateSpecialKey(key);
    if (pointer && key.repeat) this.startSpecialRepeat(key);
  }

  private activateSpecialKey(key: SpecialKey): void {
    if (key.latch) {
      this.setCtrlLatched(!this.ctrlLatched);
      return;
    }
    this.emitSpecialKey(key.value);
  }

  private startSpecialRepeat(key: SpecialKey): void {
    this.stopSpecialRepeat();
    const schedule = (delay: number): void => {
      this.specialRepeatTimer = window.setTimeout(() => {
        this.activateSpecialKey(key);
        schedule(SPECIAL_REPEAT_INTERVAL_MS);
      }, delay);
    };
    schedule(SPECIAL_REPEAT_DELAY_MS);
  }

  private stopSpecialRepeat(): void {
    if (this.specialRepeatTimer !== null) {
      window.clearTimeout(this.specialRepeatTimer);
      this.specialRepeatTimer = null;
    }
  }

  private setCtrlLatched(on: boolean): void {
    this.ctrlLatched = on;
    if (!this.ctrlLatchButton) return;
    this.ctrlLatchButton.classList.toggle('terminal-mobile-special-key--latched', on);
    this.ctrlLatchButton.setAttribute('aria-pressed', String(on));
  }

  /**
   * Ctrl latch: the newly typed character becomes its control code (letter & 0x1f) and is not kept
   * in the field. Returns false (leaving the input alone) when the character is not one of a-z,
   * @[\]^_ or when the insert cannot be located.
   */
  private consumeCtrlLatch(event: InputEvent): boolean {
    const typed = event.data ?? '';
    const control = controlCodeFor(typed);
    if (control === null) return false;

    const value = this.input.value;
    const caret = this.input.selectionStart ?? value.length;
    let at = caret - typed.length;
    if (value.slice(at, caret) !== typed) {
      if (!value.endsWith(typed)) return false;
      at = value.length - typed.length;
    }
    this.input.value = value.slice(0, at) + value.slice(at + typed.length);
    this.input.setSelectionRange(at, at);
    this.setCtrlLatched(false);
    this.flushToTerminal();
    this.emit(control);
    this.notifyBufferChange();
    return true;
  }

  private toggleSpecialPanel(): void {
    if (this.specialPanelOpen) {
      this.closeSpecialPanel();
    } else {
      this.openSpecialPanel();
    }
  }

  private openSpecialPanel(): void {
    if (this.specialPanelOpen) return;
    this.cancelSpecialPanelClose();
    this.specialPanelOpen = true;
    this.specialPanel.removeAttribute('hidden');
    this.specialControls.classList.add('terminal-mobile-special-controls--open');
    this.specialToggleButton.setAttribute('aria-expanded', 'true');
    this.specialToggleButton.setAttribute('aria-label', 'Hide special keys');
    document.addEventListener('pointerdown', this.onDocumentPointerDown, true);
    document.addEventListener('keydown', this.onDocumentKeyDown);
  }

  private closeSpecialPanel(): void {
    if (!this.specialPanelOpen) return;
    this.specialPanelOpen = false;
    this.stopSpecialRepeat();
    this.specialControls.classList.remove('terminal-mobile-special-controls--open');
    this.specialToggleButton.setAttribute('aria-expanded', 'false');
    this.specialToggleButton.setAttribute('aria-label', 'Show special keys');
    document.removeEventListener('pointerdown', this.onDocumentPointerDown, true);
    document.removeEventListener('keydown', this.onDocumentKeyDown);

    // Keep the panel mounted while it animates out, then hide it.
    this.specialPanel.classList.add('terminal-mobile-special-panel--closing');
    this.specialPanelCloseTimer = window.setTimeout(() => {
      this.specialPanelCloseTimer = null;
      this.specialPanel.classList.remove('terminal-mobile-special-panel--closing');
      this.specialPanel.setAttribute('hidden', 'true');
    }, SPECIAL_PANEL_CLOSE_MS);
  }

  private cancelSpecialPanelClose(): void {
    if (this.specialPanelCloseTimer !== null) {
      window.clearTimeout(this.specialPanelCloseTimer);
      this.specialPanelCloseTimer = null;
    }
    this.specialPanel.classList.remove('terminal-mobile-special-panel--closing');
  }

  /** A tap anywhere outside the keys closes the panel; the tap itself still reaches the terminal. */
  private readonly onDocumentPointerDown = (event: PointerEvent): void => {
    if (event.target instanceof Node && this.specialControls.contains(event.target)) return;
    this.closeSpecialPanel();
  };

  private readonly onDocumentKeyDown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') this.closeSpecialPanel();
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
    if (this.ctrlLatched && this.consumeCtrlLatch(inputEvent)) return;

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

    if (event.key === 'Escape' && this.specialPanelOpen) {
      // Escape first closes the keys panel; it is not sent to the terminal.
      event.preventDefault();
      this.closeSpecialPanel();
      return;
    }

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

/** Where a pointer, mouse or (single) touch event happened. */
function eventPoint(event: Event): { x: number; y: number } | null {
  const touches = (event as TouchEvent).changedTouches ?? (event as TouchEvent).touches;
  if (touches && touches.length > 0) return { x: touches[0].clientX, y: touches[0].clientY };
  if (typeof (event as MouseEvent).clientX === 'number') {
    return { x: (event as MouseEvent).clientX, y: (event as MouseEvent).clientY };
  }
  return null;
}
