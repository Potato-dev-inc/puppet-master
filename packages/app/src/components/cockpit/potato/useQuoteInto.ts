// The composer's end of "Reply": a message's Reply button sends POTATO_QUOTE_EVENT; this hook puts the quote in front of
// the draft and moves the cursor to the end. Minimal on purpose: the composer phase can swap it for a reply strip.
import { useEffect, type RefObject } from 'react';
import { POTATO_QUOTE_EVENT, withQuote, type PotatoQuoteDetail } from '../../../lib/potato-thread';

export function useQuoteInto(botId: string, setDraft: (update: (was: string) => string) => void, field: RefObject<HTMLTextAreaElement | null>): void {
  useEffect(() => {
    const onQuote = (event: Event) => {
      const detail = (event as CustomEvent<PotatoQuoteDetail>).detail;
      if (!detail || detail.botId !== botId) return;
      setDraft((was) => withQuote(was, detail.quote));
      const el = field.current;
      if (el) {
        el.focus();
        // After React puts the new text in, the cursor goes to the end.
        requestAnimationFrame(() => el.setSelectionRange(el.value.length, el.value.length));
      }
    };
    window.addEventListener(POTATO_QUOTE_EVENT, onQuote);
    return () => window.removeEventListener(POTATO_QUOTE_EVENT, onQuote);
  }, [botId, setDraft, field]);
}
