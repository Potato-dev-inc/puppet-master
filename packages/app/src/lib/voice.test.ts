// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import {
  browserSpeechCtor,
  checkOnDeviceVoice,
  insertAtCursor,
  startVoiceSession,
  voiceLanguage,
  type SpeechRecognitionCtor,
  type SpeechRecognitionLike,
} from './voice';

/** A recognizer that can run on the device (`processLocally` exists) and says what `available` should answer. */
function ctorWith(answer: string | 'throw' | null, opts: { processLocally?: boolean } = {}): SpeechRecognitionCtor {
  const withLocal = opts.processLocally ?? true;
  class Fake {
    lang = '';
    continuous = false;
    interimResults = false;
    onresult = null;
    onerror = null;
    onend = null;
    start() {}
    stop() {}
    abort() {}
    constructor() {
      // Only a browser that has the switch gets the property.
      if (withLocal) (this as { processLocally?: boolean }).processLocally = false;
    }
  }
  if (answer !== null) {
    (Fake as unknown as { available: unknown }).available = vi.fn(async () => {
      if (answer === 'throw') throw new Error('no');
      return answer;
    });
  }
  return Fake as unknown as SpeechRecognitionCtor;
}

describe('checkOnDeviceVoice', () => {
  it('no recognizer at all: no-api', async () => {
    expect(await checkOnDeviceVoice(null, 'en-US')).toEqual({ ok: false, reason: 'no-api' });
  });

  it('a recognizer without the on-device switch: no-on-device', async () => {
    const ctor = ctorWith('available', { processLocally: false });
    expect(await checkOnDeviceVoice(ctor, 'en-US')).toEqual({ ok: false, reason: 'no-on-device' });
  });

  it('a recognizer that cannot say if a language is on the device: no-on-device', async () => {
    const ctor = ctorWith(null);
    expect(await checkOnDeviceVoice(ctor, 'en-US')).toEqual({ ok: false, reason: 'no-on-device' });
  });

  it('on-device "available" for the language: ok, asked with processLocally true', async () => {
    const ctor = ctorWith('available');
    expect(await checkOnDeviceVoice(ctor, 'en-US')).toEqual({ ok: true });
    const available = (ctor as unknown as { available: ReturnType<typeof vi.fn> }).available;
    expect(available).toHaveBeenCalledWith({ langs: ['en-US'], processLocally: true });
  });

  it('a language that still has to be downloaded does not count: not-installed', async () => {
    expect(await checkOnDeviceVoice(ctorWith('downloadable'), 'en-US')).toEqual({ ok: false, reason: 'not-installed' });
    expect(await checkOnDeviceVoice(ctorWith('unavailable'), 'en-US')).toEqual({ ok: false, reason: 'not-installed' });
  });

  it('a check that throws: check-failed', async () => {
    expect(await checkOnDeviceVoice(ctorWith('throw'), 'en-US')).toEqual({ ok: false, reason: 'check-failed' });
  });
});

describe('browserSpeechCtor', () => {
  it('reads the standard class, then the WebKit one, else null', () => {
    const std = class {} as unknown as SpeechRecognitionCtor;
    const webkit = class {} as unknown as SpeechRecognitionCtor;
    expect(browserSpeechCtor({ SpeechRecognition: std, webkitSpeechRecognition: webkit })).toBe(std);
    expect(browserSpeechCtor({ webkitSpeechRecognition: webkit })).toBe(webkit);
    expect(browserSpeechCtor({})).toBeNull();
  });
});

describe('voiceLanguage', () => {
  it("uses the browser's language, else English", () => {
    expect(voiceLanguage({ language: 'de-DE' })).toBe('de-DE');
    expect(voiceLanguage({ language: '  ' })).toBe('en-US');
    expect(voiceLanguage(undefined)).toBe('en-US');
  });
});

describe('startVoiceSession', () => {
  it('refuses to run when the browser will not switch to the device', () => {
    class Stuck {
      lang = '';
      continuous = false;
      interimResults = false;
      onresult = null;
      onerror = null;
      onend = null;
      constructor() {
        Object.defineProperty(this, 'processLocally', { get: () => false, set: () => {} });
      }
      start() {
        throw new Error('must not start');
      }
      stop() {}
      abort() {}
    }
    const handlers = { onInterim: vi.fn(), onFinal: vi.fn(), onError: vi.fn(), onEnd: vi.fn() };
    expect(() => startVoiceSession(Stuck as unknown as SpeechRecognitionCtor, 'en-US', handlers)).toThrow();
  });

  it('turns on-device, starts, and splits final from interim text', () => {
    const made: Array<{ processLocally?: boolean; lang: string; started: number; rec: SpeechRecognitionLike }> = [];
    class Rec {
      lang = '';
      continuous = false;
      interimResults = false;
      processLocally = false;
      onresult = null;
      onerror = null;
      onend = null;
      started = 0;
      constructor() {
        made.push(this as unknown as (typeof made)[number]);
      }
      start() {
        this.started += 1;
      }
      stop() {}
      abort() {}
    }
    const handlers = { onInterim: vi.fn(), onFinal: vi.fn(), onError: vi.fn(), onEnd: vi.fn() };
    startVoiceSession(Rec as unknown as SpeechRecognitionCtor, 'fr-FR', handlers);
    const rec = made[0] as unknown as SpeechRecognitionLike & { started: number };
    expect(rec.processLocally).toBe(true);
    expect(rec.lang).toBe('fr-FR');
    expect(rec.continuous).toBe(true);
    expect(rec.interimResults).toBe(true);
    expect(rec.started).toBe(1);

    const result = (text: string, isFinal: boolean) =>
      Object.assign([{ transcript: text }], { isFinal }) as unknown as { isFinal: boolean; 0: { transcript: string } };
    rec.onresult?.({ resultIndex: 0, results: [result('hello ', true), result('wor', false)] });
    expect(handlers.onFinal).toHaveBeenCalledWith('hello ');
    expect(handlers.onInterim).toHaveBeenCalledWith('wor');
  });
});

describe('insertAtCursor', () => {
  it('puts words at the cursor with one space on each side where needed', () => {
    expect(insertAtCursor('Fix the test', 4, 4, 'login')).toEqual({ value: 'Fix login the test', caret: 9 });
    expect(insertAtCursor('', 0, 0, ' hello there ')).toEqual({ value: 'hello there', caret: 11 });
    expect(insertAtCursor('one', 3, 3, 'two')).toEqual({ value: 'one two', caret: 7 });
  });

  it('replaces a selected range and ignores an empty chunk', () => {
    expect(insertAtCursor('abc xyz', 4, 7, 'new')).toEqual({ value: 'abc new', caret: 7 });
    expect(insertAtCursor('keep', 2, 2, '   ')).toEqual({ value: 'keep', caret: 2 });
  });
});
