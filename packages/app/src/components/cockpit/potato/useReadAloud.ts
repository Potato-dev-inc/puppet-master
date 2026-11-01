// "Read replies aloud" for one bot: each new reply of a bot that has the switch on is read as it arrives, while the
// page is visible. Replies that were already in the thread when the page opened are never read, and a reply that
// arrives while the page is hidden is skipped (it is marked as seen, so it is not read later either).
import { useEffect, useRef } from 'react';
import type { PotatoMessage, PotatoVoice } from '../../../lib/potato';
import { speakable, speakReply, speechSupported, normalizeVoice } from '../../../lib/potato-speech';

export function useReadAloud(botId: string, messages: PotatoMessage[], voice: PotatoVoice | undefined): void {
  const openedAt = useRef(Date.now());
  const handled = useRef(new Set<string>());

  useEffect(() => {
    const fresh = messages.filter((m) => !handled.current.has(m.id));
    for (const m of fresh) handled.current.add(m.id);
    if (!voice?.readAloudBots.includes(botId)) return;
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return;
    if (!speechSupported()) return;
    const prefs = normalizeVoice(voice);
    for (const m of fresh) {
      if (m.from.kind === 'bot' && m.kind === 'text' && m.at >= openedAt.current && speakable(m.text)) {
        speakReply(m.id, m.text, prefs, false);
      }
    }
  }, [botId, messages, voice]);
}
