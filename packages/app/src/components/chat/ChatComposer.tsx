// The message box under the Chat tab: write a message for the agent. Enter sends, Shift+Enter makes a new line.
// The message is not typed at once: it waits until the agent is free (the pane outbox in the backend), and shows
// as a bubble under the conversation meanwhile (PendingMessages).
// `voice` adds the mic (phone only): speech turned into text on the device, see hooks/useVoiceInput.ts.
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';
import { MAX_MESSAGE_CHARS } from '../../lib/paneOutbox';
import { insertAtCursor } from '../../lib/voice';
import { useVoiceInput } from '../../hooks/useVoiceInput';

/** The box grows with the text up to this many lines, then scrolls. */
const MAX_LINES = 6;
/** Show the character count from here on. */
const COUNT_FROM = 7000;

export const TOO_LONG_TEXT = `The message is too long. The most is ${MAX_MESSAGE_CHARS} characters.`;

export interface ChatComposerProps {
  /** Sends the text. Resolves to null when it was accepted, else one plain sentence saying why not. */
  onSend: (text: string) => Promise<string | null>;
  /** Why the box is switched off (shown instead of a hint), or null. */
  blockedReason: string | null;
  /** Show the mic (voice typing on the device). Off for the desktop box. */
  voice?: boolean;
}

function MicIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0" />
      <path d="M12 18v3" />
    </svg>
  );
}

export function ChatComposer({ onSend, blockedReason, voice = false }: ChatComposerProps) {
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Words heard but not final yet, and where in the text they would go. */
  const [interim, setInterim] = useState('');
  const [interimAt, setInterimAt] = useState(0);
  const field = useRef<HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  /** Where the cursor goes after the next render (set after inserting recognized words). */
  const caretAfter = useRef<number | null>(null);
  const blocked = blockedReason !== null;
  const count = [...text].length;
  const tooLong = count > MAX_MESSAGE_CHARS;
  const empty = text.trim() === '';

  const mic = useVoiceInput({
    enabled: voice,
    onInterim: (heard) => {
      setInterim(heard);
      setInterimAt(field.current?.selectionStart ?? text.length);
    },
    onFinal: (chunk) => {
      const el = field.current;
      const value = el ? el.value : '';
      const start = el ? el.selectionStart : value.length;
      const end = el ? el.selectionEnd : value.length;
      const next = insertAtCursor(value, start, end, chunk);
      caretAfter.current = next.caret;
      setText(next.value);
      setInterim('');
    },
  });
  const micOff = blocked || busy || mic.support === 'checking';

  // Grow with the text, up to MAX_LINES lines.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = 'auto';
    const line = parseFloat(getComputedStyle(el).lineHeight);
    const max = (Number.isFinite(line) ? line : 20) * MAX_LINES;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [text]);

  // Put the cursor after recognized words, once the text has been rendered.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el || caretAfter.current === null) return;
    el.setSelectionRange(caretAfter.current, caretAfter.current);
    caretAfter.current = null;
  }, [text]);

  const submit = async () => {
    if (blocked || busy || empty || tooLong) return;
    mic.halt();
    setBusy(true);
    setError(null);
    const refused = await onSend(text.trim());
    setBusy(false);
    if (refused) setError(refused);
    else setText('');
    field.current?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter while an input method is choosing a word is the method's, not ours.
    if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return;
    event.preventDefault();
    void submit();
  };

  // The mic: a press starts (hold to talk, or tap to start and tap to stop). Focus stays in the box.
  const onMicDown = (event: PointerEvent<HTMLButtonElement>) => {
    event.preventDefault();
    if (!micOff) mic.pressStart();
  };
  const onMicUp = () => mic.pressEnd();
  const onMicKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if ((event.key !== 'Enter' && event.key !== ' ') || event.repeat) return;
    event.preventDefault();
    if (!micOff) mic.tap();
  };

  const note = blockedReason ?? (tooLong ? TOO_LONG_TEXT : error);
  const showMirror = voice && interim !== '';
  const at = Math.min(interimAt, text.length);

  const textarea = (
    <textarea
      ref={field}
      rows={1}
      value={text}
      disabled={blocked}
      aria-label="Message to the agent"
      placeholder={blocked ? '' : 'Write a message for the agent'}
      onChange={(event) => {
        setText(event.target.value);
        if (error) setError(null);
      }}
      onKeyDown={onKeyDown}
      onScroll={(event) => {
        if (mirror.current) mirror.current.scrollTop = event.currentTarget.scrollTop;
      }}
      className={showMirror ? 'ck-chat-composer-field ck-chat-composer-field--voice' : 'ck-chat-composer-field'}
    />
  );

  return (
    <form
      className="ck-chat-composer"
      data-testid="chat-composer"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <div className="ck-chat-composer-row">
        {voice ? (
          <div className="ck-chat-composer-field-wrap">
            {showMirror && (
              <div ref={mirror} className="ck-chat-composer-mirror" aria-hidden="true" data-testid="chat-composer-mirror">
                {text.slice(0, at)}
                <span className="ck-chat-composer-interim">{interim}</span>
                {text.slice(at)}
                {' '}
              </div>
            )}
            {textarea}
          </div>
        ) : (
          textarea
        )}
        {voice && (
          <button
            type="button"
            className="ck-chat-mic"
            data-testid="chat-composer-mic"
            aria-label={mic.support === 'hint' ? "Voice typing: use your keyboard's mic" : 'Voice typing'}
            aria-pressed={mic.recording}
            aria-disabled={micOff || undefined}
            onPointerDown={onMicDown}
            onPointerUp={onMicUp}
            onPointerCancel={onMicUp}
            onPointerLeave={onMicUp}
            onKeyDown={onMicKey}
          >
            <MicIcon />
            {mic.recording && <span className="ck-chat-mic-dot" aria-hidden="true" data-testid="chat-composer-mic-dot" />}
          </button>
        )}
        <button type="submit" className="ck-btn ck-btn--primary ck-chat-send" disabled={blocked || busy || empty || tooLong}>
          Send
        </button>
      </div>
      {voice && mic.note && (
        <p className="ck-chat-composer-note ck-chat-composer-voice" role="status" data-testid="chat-composer-voice">
          {mic.note}
        </p>
      )}
      {(note || count >= COUNT_FROM) && (
        <p className="ck-chat-composer-note" role={blocked ? undefined : 'alert'} data-testid="chat-composer-note">
          {note}
          {!blocked && count >= COUNT_FROM && (
            <span className="ck-chat-composer-count">
              {note ? ' · ' : ''}
              {count} / {MAX_MESSAGE_CHARS}
            </span>
          )}
        </p>
      )}
    </form>
  );
}
