// Which of a pane's outbox messages show as bubbles under the conversation (see `bubblesFor`): the clock that ends
// a sent message's bubble after a few seconds, and the failed ones the person hid.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { bubblesFor, SENT_BUBBLE_MS, type OutboxMessage } from '../lib/paneOutbox';

export function usePendingBubbles(messages: readonly OutboxMessage[], chatUserTexts: readonly string[]) {
  const sentSeen = useRef(new Map<string, number>());
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const [now, setNow] = useState(() => Date.now());

  // First time a message is seen as sent starts its few seconds.
  const stamp = Date.now();
  for (const m of messages) {
    if (m.state === 'sent' && !sentSeen.current.has(m.id)) sentSeen.current.set(m.id, stamp);
  }

  // Wake up when the oldest still-shown sent bubble is due to go.
  const due = useMemo(() => {
    let next: number | null = null;
    for (const m of messages) {
      const seen = sentSeen.current.get(m.id);
      if (m.state === 'sent' && seen !== undefined && !hidden.has(m.id)) next = Math.min(next ?? Infinity, seen + SENT_BUBBLE_MS);
    }
    return next;
  }, [messages, hidden, now]);
  useEffect(() => {
    if (due === null) return;
    const wait = Math.max(0, due - Date.now()) + 20;
    const timer = setTimeout(() => setNow(Date.now()), wait);
    return () => clearTimeout(timer);
  }, [due]);

  const bubbles = useMemo(
    () => bubblesFor(messages, chatUserTexts, sentSeen.current, hidden, Math.max(now, Date.now())),
    [messages, chatUserTexts, hidden, now],
  );
  const hide = useCallback((id: string) => setHidden((prev) => new Set(prev).add(id)), []);
  return { bubbles, hide };
}
