// @vitest-environment jsdom
// Voice typing in the phone's message box, with a fake SpeechRecognition standing in for the browser.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatComposer } from './ChatComposer';
import { KEYBOARD_MIC_HINT, KEYBOARD_MIC_TIP } from '../../lib/voice';
import { HOLD_MS, LISTENING_NOTE } from '../../hooks/useVoiceInput';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Answer = 'available' | 'unavailable' | 'downloadable';

/**
 * A fake browser recognizer. `checks` are the on-device questions asked; `sessions` are the recognitions that
 * were actually started (the check makes a throwaway instance, which is not a session).
 */
class FakeRecognition {
  static sessions: FakeRecognition[] = [];
  static answer: Answer | 'none' = 'available';
  static checks: Array<{ langs: string[]; processLocally: boolean }> = [];

  static available = async (options: { langs: string[]; processLocally: boolean }) => {
    FakeRecognition.checks.push(options);
    if (FakeRecognition.answer === 'none') return 'unavailable';
    // Cloud-only browsers answer "available" for the cloud recognizer and not on the device.
    return options.processLocally ? FakeRecognition.answer : 'available';
  };

  lang = '';
  continuous = false;
  interimResults = false;
  processLocally = false;
  onresult: ((event: unknown) => void) | null = null;
  onerror: ((event: { error: string }) => void) | null = null;
  onend: (() => void) | null = null;
  starts = 0;
  stops = 0;
  aborts = 0;

  start() {
    this.starts += 1;
    FakeRecognition.sessions.push(this);
  }
  stop() {
    this.stops += 1;
  }
  abort() {
    this.aborts += 1;
  }
  /** Speak: a list of [text, isFinal] results, as the browser sends them. */
  emit(results: Array<[string, boolean]>) {
    const list = results.map(([transcript, isFinal]) => Object.assign([{ transcript }], { isFinal }));
    this.onresult?.({ resultIndex: 0, results: list });
  }
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  FakeRecognition.sessions = [];
  FakeRecognition.checks = [];
  FakeRecognition.answer = 'available';
  vi.stubGlobal('SpeechRecognition', FakeRecognition);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  Reflect.deleteProperty(document, 'visibilityState');
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const field = () => host.querySelector('textarea') as HTMLTextAreaElement;
const mic = () => host.querySelector('[data-testid="chat-composer-mic"]') as HTMLButtonElement;
const voiceNote = () => host.querySelector('[data-testid="chat-composer-voice"]')?.textContent ?? null;
const mirror = () => host.querySelector('[data-testid="chat-composer-mirror"]');

function pointer(type: 'pointerdown' | 'pointerup') {
  act(() => {
    mic().dispatchEvent(new Event(type, { bubbles: true, cancelable: true }));
  });
}

/** Press and release the mic: a short tap (starts or stops) or a hold of `ms`. */
function press(ms: number) {
  const now = Date.now();
  vi.spyOn(Date, 'now').mockReturnValue(now);
  pointer('pointerdown');
  vi.spyOn(Date, 'now').mockReturnValue(now + ms);
  pointer('pointerup');
  vi.restoreAllMocks();
}

function renderComposer(onSend = vi.fn(async () => null), voice = true) {
  act(() => root.render(<ChatComposer onSend={onSend} blockedReason={null} voice={voice} />));
  return onSend;
}

/** Types text into the box, the way a person does. */
function typeText(text: string) {
  const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    set.call(field(), text);
    field().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

function setCursor(at: number) {
  act(() => field().setSelectionRange(at, at));
}

describe('ChatComposer voice typing', () => {
  it('on-device recognition is available: records, shows gray interim text, inserts final text at the cursor, sends nothing', async () => {
    const onSend = renderComposer();
    await flush();
    expect(FakeRecognition.checks).toEqual([{ langs: [navigator.language || 'en-US'], processLocally: true }]);
    expect(voiceNote()).toBeNull();

    typeText('Fix the test');
    setCursor(4);
    press(50); // a short tap starts
    await flush();

    const rec = FakeRecognition.sessions[0];
    expect(rec.starts).toBe(1);
    expect(rec.processLocally).toBe(true);
    expect(mic().getAttribute('aria-pressed')).toBe('true');
    expect(host.querySelector('[data-testid="chat-composer-mic-dot"]')).not.toBeNull();
    expect(voiceNote()).toBe(LISTENING_NOTE);

    act(() => rec.emit([['login', false]]));
    expect(mirror()?.querySelector('.ck-chat-composer-interim')?.textContent).toBe('login');
    expect(mirror()?.textContent?.startsWith('Fix ')).toBe(true);
    expect(field().className).toContain('ck-chat-composer-field--voice');
    expect(field().value).toBe('Fix the test');

    act(() => rec.emit([['login', true]]));
    expect(field().value).toBe('Fix login the test');
    expect(mirror()).toBeNull();
    expect(field().selectionStart).toBe('Fix login'.length);
    expect(onSend).not.toHaveBeenCalled();
  });

  it('a short tap keeps recording; the next tap stops it', async () => {
    renderComposer();
    await flush();
    press(50);
    await flush();
    const rec = FakeRecognition.sessions[0];
    expect(rec.stops).toBe(0);
    press(50); // second tap stops
    expect(rec.stops).toBe(1);
  });

  it('hold to talk: releasing after the hold time stops', async () => {
    renderComposer();
    await flush();
    press(HOLD_MS + 100);
    const rec = FakeRecognition.sessions[0];
    expect(rec.starts).toBe(1);
    expect(rec.stops).toBe(1);
  });

  it('stops when the page is hidden (drops what is heard at once)', async () => {
    renderComposer();
    await flush();
    press(50);
    const rec = FakeRecognition.sessions[0];
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'));
    });
    expect(rec.aborts).toBe(1);
    expect(mic().getAttribute('aria-pressed')).toBe('false');
  });

  it('stops when the page is closed or the window loses focus', async () => {
    renderComposer();
    await flush();
    press(50);
    const first = FakeRecognition.sessions[0];
    act(() => {
      window.dispatchEvent(new Event('pagehide'));
    });
    expect(first.aborts).toBe(1);

    press(50); // a new session starts
    const second = FakeRecognition.sessions[1];
    expect(second.starts).toBe(1);
    act(() => {
      window.dispatchEvent(new Event('blur'));
    });
    expect(second.stops).toBe(1);
  });

  it('only cloud recognition available: shows the keyboard hint, never starts', async () => {
    FakeRecognition.answer = 'unavailable';
    renderComposer();
    await flush();
    expect(voiceNote()).toBe(KEYBOARD_MIC_HINT);
    expect(mic().getAttribute('aria-label')).toBe("Voice typing: use your keyboard's mic");

    press(50);
    expect(voiceNote()).toBe(KEYBOARD_MIC_TIP);
    press(HOLD_MS + 100);
    expect(FakeRecognition.sessions).toHaveLength(0);
    expect(FakeRecognition.checks.every((check) => check.processLocally === true)).toBe(true);
  });

  it('a language that still has to be downloaded counts as no on-device recognition', async () => {
    FakeRecognition.answer = 'downloadable';
    renderComposer();
    await flush();
    expect(voiceNote()).toBe(KEYBOARD_MIC_HINT);
    press(50);
    expect(FakeRecognition.sessions).toHaveLength(0);
  });

  it('no speech recognition at all: shows the keyboard hint, never starts', async () => {
    vi.unstubAllGlobals();
    renderComposer();
    await flush();
    expect(voiceNote()).toBe(KEYBOARD_MIC_HINT);
    press(50);
    expect(voiceNote()).toBe(KEYBOARD_MIC_TIP);
    expect(FakeRecognition.sessions).toHaveLength(0);
  });

  it('a blocked mic shows one plain line, and the next tap only shows the keyboard tip', async () => {
    renderComposer();
    await flush();
    press(50);
    const rec = FakeRecognition.sessions[0];
    act(() => rec.onerror?.({ error: 'not-allowed' }));
    expect(voiceNote()).toBe('The mic is blocked. Allow the mic for this site in your browser settings.');
    press(50);
    expect(voiceNote()).toBe(KEYBOARD_MIC_TIP);
    expect(FakeRecognition.sessions).toHaveLength(1);
  });

  it('the desktop box has no mic', async () => {
    renderComposer(vi.fn(async () => null), false);
    await flush();
    expect(host.querySelector('[data-testid="chat-composer-mic"]')).toBeNull();
    expect(FakeRecognition.checks).toHaveLength(0);
  });
});
