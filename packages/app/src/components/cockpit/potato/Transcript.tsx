// The message list of a bot's thread: the rows, the typing row, and the scrolling. It follows the bottom while the
// person is at the bottom; reading further up is left alone. `followToken` goes up when the person sends, which
// pulls the view to the bottom again.
import { useEffect, useLayoutEffect, useRef } from 'react';
import type { PotatoBotView, PotatoMessage, PotatoTransport, PotatoVoice } from '../../../lib/potato';
import { THREAD_COPY } from '../../../lib/potato-thread';
import { MessageRow } from './MessageRow';
import { TypingRow } from './TypingRow';
import { threadRows } from './model';
import '../../../styles/cockpit-potato-thread.css';

/** Closer than this to the bottom, a new line keeps the thread pinned to it. */
const STICK_PX = 120;

export interface TranscriptProps {
  bot: PotatoBotView;
  messages: PotatoMessage[];
  agentLabel: string;
  transport: PotatoTransport;
  onChanged: () => void;
  onOpenPane?: (paneId: string) => void;
  paneExists?: (paneId: string) => boolean;
  followToken?: number;
  now: number;
  /** The voice settings, for a reply's Speak button. */
  voice?: PotatoVoice;
}

export function Transcript({ bot, messages, agentLabel, transport, onChanged, onOpenPane, paneExists, followToken = 0, now, voice }: TranscriptProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    pinned.current = true;
  }, [bot.id, followToken]);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [messages, bot.typing, followToken]);

  return (
    <div
      className="ck-scroll ck-potato-body"
      ref={scroller}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_PX;
      }}
    >
      <ol className="ck-potato-rows" aria-label={THREAD_COPY.messages}>
        {threadRows(messages).map((row) => (
          <MessageRow
            key={row.message.id}
            row={row}
            botId={bot.id}
            botName={bot.name}
            agentLabel={agentLabel}
            transport={transport}
            onChanged={onChanged}
            onOpenPane={onOpenPane}
            paneExists={paneExists}
            now={now}
            voice={voice}
          />
        ))}
      </ol>
      {bot.typing && <TypingRow name={bot.name} />}
    </div>
  );
}
