// The message box at the bottom of a bot's thread: a growing textarea and Send. Enter sends; Shift+Enter adds a line.
// The composer phase replaces the inside of this file (pill, attach, drafts); the props stay.
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { COPY, plainError, type PotatoBotView, type PotatoTransport } from '../../../lib/potato';
import { MAX_MESSAGE } from '../../../lib/potato-composer';
import { canWrite } from './model';
import { useQuoteInto } from './useQuoteInto';
import '../../../styles/cockpit-potato-composer.css';

export interface ComposerProps {
  bot: PotatoBotView;
  transport: PotatoTransport;
  /** Called after a send, so the state is read again at once. */
  onChanged: () => void;
  /** Said when the send fails. */
  onNotify: (text: string) => void;
  /** The message went out: the thread follows the bottom again. */
  onSent?: () => void;
}

export function Composer({ bot, transport, onChanged, onNotify, onSent }: ComposerProps) {
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  // "Reply" on a message puts a quote in the box.
  useQuoteInto(bot.id, setDraft, field);

  // The box grows with the text, up to about six lines.
  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 132)}px`;
  }, [draft]);

  const send = async () => {
    const text = draft.trim();
    if (!text || sending || !canWrite(bot.brainState)) return;
    setSending(true);
    setDraft('');
    onSent?.();
    try {
      await transport.send(bot.id, text);
      onChanged();
    } catch (error) {
      setDraft(text);
      onNotify(plainError(error, 'Could not send that.'));
    } finally {
      setSending(false);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== 'Enter' || e.shiftKey || e.nativeEvent.isComposing) return;
    e.preventDefault();
    void send();
  };

  return (
    <form
      className="ck-chat-composer ck-potato-composer"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <div className="ck-chat-composer-row">
        <textarea
          ref={field}
          className="ck-chat-composer-field"
          aria-label={COPY.composer(bot.name)}
          placeholder={COPY.composer(bot.name)}
          rows={1}
          maxLength={MAX_MESSAGE}
          value={draft}
          disabled={!canWrite(bot.brainState)}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKey}
        />
        <button type="submit" className="ck-btn ck-btn--primary ck-chat-send" disabled={sending || !draft.trim() || !canWrite(bot.brainState)}>
          Send
        </button>
      </div>
    </form>
  );
}
