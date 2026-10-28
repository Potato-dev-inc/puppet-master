// The mic of the phone's message box: checks once that the browser can recognize speech on the device, then
// records on press (hold to talk, or tap to start and tap again to stop). Text goes out through callbacks; this
// hook never sends anything. Recording stops when the window loses focus, the page is hidden, or the view unmounts.
import { useEffect, useRef, useState } from 'react';
import {
  browserSpeechCtor,
  checkOnDeviceVoice,
  KEYBOARD_MIC_HINT,
  KEYBOARD_MIC_TIP,
  startVoiceSession,
  voiceLanguage,
  type SpeechRecognitionCtor,
  type VoiceSession,
} from '../lib/voice';

/** A press at least this long (ms) is "hold to talk": release stops. A shorter tap leaves recording on. */
export const HOLD_MS = 400;
/** Listening line shown while recording. */
export const LISTENING_NOTE = 'Listening. Tap the mic to stop.';

/** Plain sentences for recognizer errors. Quiet ones (no speech, aborted) show nothing. */
const ERROR_NOTES: Record<string, string> = {
  'not-allowed': 'The mic is blocked. Allow the mic for this site in your browser settings.',
  'service-not-allowed': 'Voice typing is not allowed on this phone.',
  'audio-capture': 'No mic was found.',
  'language-not-supported': 'Voice typing does not know this language on this phone.',
};

export type VoiceSupport = 'checking' | 'on-device' | 'hint';

export interface VoiceInput {
  support: VoiceSupport;
  recording: boolean;
  /** One plain line for the box, or null. */
  note: string | null;
  /** Pointer down: start (hold to talk), or stop when recording. */
  pressStart(): void;
  /** Pointer up or cancel: a hold that lasted long enough stops. */
  pressEnd(): void;
  /** Keyboard: start, or stop when recording. */
  tap(): void;
  /** Finish: the last words still arrive, then the session ends. */
  stop(): void;
  /** Drop everything heard and end now. */
  halt(): void;
}

interface Options {
  enabled: boolean;
  onInterim: (text: string) => void;
  onFinal: (text: string) => void;
}

export function useVoiceInput({ enabled, onInterim, onFinal }: Options): VoiceInput {
  const [support, setSupport] = useState<VoiceSupport>('checking');
  const [recording, setRecording] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const supportRef = useRef<VoiceSupport>('checking');
  const sessionRef = useRef<VoiceSession | null>(null);
  const seqRef = useRef(0);
  const holdRef = useRef<number | null>(null);
  const ctorRef = useRef<SpeechRecognitionCtor | null>(null);
  const cbRef = useRef({ onInterim, onFinal });
  cbRef.current = { onInterim, onFinal };

  const setSupportBoth = (next: VoiceSupport) => {
    supportRef.current = next;
    setSupport(next);
  };

  const finish = () => {
    sessionRef.current = null;
    holdRef.current = null;
    setRecording(false);
    cbRef.current.onInterim('');
  };

  const start = () => {
    const ctor = ctorRef.current;
    if (!ctor || sessionRef.current) return;
    const id = ++seqRef.current;
    setNote(null);
    try {
      sessionRef.current = startVoiceSession(ctor, voiceLanguage(), {
        onInterim: (text) => cbRef.current.onInterim(text),
        onFinal: (text) => cbRef.current.onFinal(text),
        onError: (code) => {
          if (seqRef.current !== id) return;
          const line = ERROR_NOTES[code];
          if (line) setNote(line);
          if (code === 'not-allowed' || code === 'service-not-allowed' || code === 'language-not-supported') {
            // Speech cannot work here: show the keyboard hint instead of trying again.
            setSupportBoth('hint');
          }
        },
        onEnd: () => {
          if (seqRef.current !== id) return;
          finish();
        },
      });
      setRecording(true);
    } catch {
      // The browser refused to run on the device: never fall back to the cloud, show the hint.
      sessionRef.current = null;
      setSupportBoth('hint');
      setNote(KEYBOARD_MIC_TIP);
    }
  };

  const stop = () => {
    holdRef.current = null;
    sessionRef.current?.stop();
  };

  const halt = () => {
    holdRef.current = null;
    sessionRef.current?.abort();
    sessionRef.current = null;
    setRecording(false);
    cbRef.current.onInterim('');
  };

  const onHint = () => {
    setNote(KEYBOARD_MIC_TIP);
  };

  const pressStart = () => {
    if (supportRef.current === 'checking') return;
    if (supportRef.current === 'hint') return onHint();
    if (sessionRef.current) {
      holdRef.current = null;
      stop();
      return;
    }
    start();
    holdRef.current = Date.now();
  };

  const pressEnd = () => {
    const at = holdRef.current;
    holdRef.current = null;
    if (at !== null && Date.now() - at >= HOLD_MS) stop();
  };

  const tap = () => {
    if (supportRef.current === 'checking') return;
    if (supportRef.current === 'hint') return onHint();
    if (sessionRef.current) stop();
    else start();
  };

  // One check per mount: can the browser recognize this language on the device?
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    ctorRef.current = browserSpeechCtor();
    void checkOnDeviceVoice(ctorRef.current, voiceLanguage()).then((check) => {
      if (!live) return;
      setSupportBoth(check.ok ? 'on-device' : 'hint');
    });
    return () => {
      live = false;
      seqRef.current += 1;
      sessionRef.current?.abort();
      sessionRef.current = null;
    };
    // The check runs once per mount; the callbacks are read through cbRef.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);

  // Stop when the person leaves: the window loses focus (graceful), or the page is hidden (drop it at once).
  useEffect(() => {
    if (!enabled) return;
    const onBlur = () => {
      if (sessionRef.current) stop();
    };
    const onHide = () => {
      if (sessionRef.current) halt();
    };
    const onVisibility = () => {
      if (document.visibilityState === 'hidden') onHide();
    };
    window.addEventListener('blur', onBlur);
    window.addEventListener('pagehide', onHide);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('pagehide', onHide);
      document.removeEventListener('visibilitychange', onVisibility);
    };
    // stop/halt only use refs, so this subscribes once per enable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);

  // A note (an error, or the tip after a tap) wins; then "Listening" while recording; then the hint.
  const line = note ?? (recording ? LISTENING_NOTE : support === 'hint' ? KEYBOARD_MIC_HINT : null);

  return { support, recording, note: enabled ? line : null, pressStart, pressEnd, tap, stop, halt };
}
