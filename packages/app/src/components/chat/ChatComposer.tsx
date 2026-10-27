// The message box under the Chat tab: write a message for the agent. Enter sends, Shift+Enter makes a new line.
// The message is not typed at once: it waits until the agent is free (the pane outbox in the backend), and shows
// as a bubble under the conversation meanwhile (PendingMessages).
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { MAX_MESSAGE_CHARS } from '../../lib/paneOutbox';

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
}

export function ChatComposer({ onSend, blockedReason }: ChatComposerProps) {
  const [text, setText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const blocked = blockedReason !== null;
  const count = [...text].length;
  const tooLong = count > MAX_MESSAGE_CHARS;
  const empty = text.trim() === '';

  // Grow with the text, up to MAX_LINES lines.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = 'auto';
    const line = parseFloat(getComputedStyle(el).lineHeight);
    const max = (Number.isFinite(line) ? line : 20) * MAX_LINES;
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
  }, [text]);

  const submit = async () => {
    if (blocked || busy || empty || tooLong) return;
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

  const note = blockedReason ?? (tooLong ? TOO_LONG_TEXT : error);

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
          className="ck-chat-composer-field"
        />
        <button type="submit" className="ck-btn ck-btn--primary ck-chat-send" disabled={blocked || busy || empty || tooLong}>
          Send
        </button>
      </div>
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
