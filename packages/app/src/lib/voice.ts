// Voice typing for the phone's message box. Speech is turned into text on the device only: the Web Speech API is
// used only when the browser can prove it works on-device (`processLocally`), never the cloud recognizer. If it
// cannot, the box shows a hint instead and nothing starts. The Web Speech types are not in TypeScript's DOM lib,
// so the small part we use is declared here.

/** The part of a speech recognition object this app uses. */
export interface SpeechRecognitionLike {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  /** Chrome's switch: true = recognize on the device only. Must be true before `start()`. */
  processLocally?: boolean;
  onresult: ((event: SpeechResultEventLike) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
}

export interface SpeechResultEventLike {
  resultIndex: number;
  results: ArrayLike<{ isFinal: boolean; 0: { transcript: string } }>;
}

export interface SpeechRecognitionCtor {
  new (): SpeechRecognitionLike;
  /** Chrome: can this language be recognized on the device right now? */
  available?: (options: { langs: string[]; processLocally: boolean }) => Promise<string>;
}

/** Why voice typing is or is not offered. `ok` = on-device recognition works for this language. */
export type VoiceCheck =
  | { ok: true }
  | { ok: false; reason: 'no-api' | 'no-on-device' | 'not-installed' | 'check-failed' };

/** The one line the box shows when voice typing is not offered. */
export const KEYBOARD_MIC_HINT = "Use your keyboard's mic";
/** What a tap on the hint shows. */
export const KEYBOARD_MIC_TIP = 'Tap the mic key on your keyboard, next to the space bar, and talk.';

/** The browser's speech recognition class, or null. Reads `globalThis` so tests can stand in a fake. */
export function browserSpeechCtor(scope: object = globalThis): SpeechRecognitionCtor | null {
  const s = scope as { SpeechRecognition?: SpeechRecognitionCtor; webkitSpeechRecognition?: SpeechRecognitionCtor };
  return s.SpeechRecognition ?? s.webkitSpeechRecognition ?? null;
}

/** The language to recognize: the browser's own (`navigator.language`), else English. */
export function voiceLanguage(nav: { language?: string } | undefined = globalThis.navigator): string {
  const lang = nav?.language?.trim();
  return lang ? lang : 'en-US';
}

/**
 * Can this browser recognize `lang` on the device? Only a clear "available" answer counts. A language that still
 * has to be downloaded does not, because starting the download is not ours to decide.
 */
export async function checkOnDeviceVoice(ctor: SpeechRecognitionCtor | null, lang: string): Promise<VoiceCheck> {
  if (!ctor) return { ok: false, reason: 'no-api' };
  let probe: SpeechRecognitionLike;
  try {
    probe = new ctor();
  } catch {
    return { ok: false, reason: 'no-api' };
  }
  if (!('processLocally' in probe) || typeof ctor.available !== 'function') {
    return { ok: false, reason: 'no-on-device' };
  }
  try {
    const status = await ctor.available({ langs: [lang], processLocally: true });
    return status === 'available' ? { ok: true } : { ok: false, reason: 'not-installed' };
  } catch {
    return { ok: false, reason: 'check-failed' };
  }
}

export interface VoiceHandlers {
  /** The text still being heard (may change many times). */
  onInterim: (text: string) => void;
  /** Recognized text that will not change any more. */
  onFinal: (text: string) => void;
  /** The recognizer error code, e.g. "not-allowed" when the mic is blocked. */
  onError: (code: string) => void;
  /** Recognition ended (stopped, or the engine gave up). */
  onEnd: () => void;
}

export interface VoiceSession {
  /** Finish: the last words still count, then `onEnd`. */
  stop(): void;
  /** Drop what is heard and end at once. */
  abort(): void;
}

/**
 * Starts one on-device recognition. Refuses (throws) if the object will not run on the device, so a cloud
 * recognizer can never be started by mistake.
 */
export function startVoiceSession(ctor: SpeechRecognitionCtor, lang: string, handlers: VoiceHandlers): VoiceSession {
  const rec = new ctor();
  rec.lang = lang;
  rec.continuous = true;
  rec.interimResults = true;
  rec.processLocally = true;
  if (rec.processLocally !== true) throw new Error('on-device speech is not available');

  rec.onresult = (event) => {
    let finals = '';
    let interim = '';
    for (let i = event.resultIndex; i < event.results.length; i += 1) {
      const result = event.results[i];
      const text = result[0]?.transcript ?? '';
      if (result.isFinal) finals += text;
      else interim += text;
    }
    if (finals) handlers.onFinal(finals);
    handlers.onInterim(interim);
  };
  rec.onerror = (event) => handlers.onError(event.error);
  rec.onend = () => handlers.onEnd();
  rec.start();

  return {
    stop: () => rec.stop(),
    abort: () => rec.abort(),
  };
}

/**
 * Puts recognized text into `value` at the cursor (`start`..`end`), with a space where two words would touch.
 * Returns the new text and where the cursor goes (just after the new words).
 */
export function insertAtCursor(value: string, start: number, end: number, chunk: string): { value: string; caret: number } {
  const piece = chunk.trim();
  const before = value.slice(0, start);
  const after = value.slice(end);
  if (!piece) return { value, caret: start };
  const lead = before === '' || /\s$/.test(before) ? '' : ' ';
  const tail = after === '' || /^\s/.test(after) ? '' : ' ';
  const words = lead + piece;
  return { value: before + words + tail + after, caret: before.length + words.length };
}
