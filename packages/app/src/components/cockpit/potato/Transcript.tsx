// The message list of a bot's thread: the rows, the lines between them (day, time, "New"), the typing row, and the
// scrolling. It follows the bottom while the person is at the bottom; reading further up is left alone, and a round
// "Jump to latest" button shows then. `followToken` goes up when the person sends, which pulls the view to the bottom
// again and takes the "New" line away.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { PotatoBotView, PotatoMessage, PotatoTransport, PotatoVoice } from '../../../lib/potato';
import { THREAD_COPY, firstUnreadId, type ToolStep } from '../../../lib/potato-thread';
import { MessageRow } from './MessageRow';
import { DaySeparator, TimeSeparator, UnreadDivider } from './Separators';
import { ThreadIcon } from './ThreadIcons';
import { ToolActivity } from './ToolActivity';
import { TypingRow } from './TypingRow';
import { threadItems } from './model';
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
  /** A brand-new bot's first message is shown above: no "Say hello" line. */
  hideEmpty?: boolean;
  now: number;
  /** Opens a link from the bot's text. Default: the system browser. */
  openLink?: (url: string) => void;
  /** The voice settings, for a reply's Speak button. */
  voice?: PotatoVoice;
  /** Sends a message as the person (a failed step's "Try again"). */
  onSendText?: (text: string) => void;
  /** Opens the bot's computer (a failed step's "Take control"). */
  onTakeControl?: () => void;
}

export function Transcript({ bot, messages, agentLabel, transport, onChanged, onOpenPane, paneExists, followToken = 0, hideEmpty = false, now, openLink, voice, onSendText, onTakeControl }: TranscriptProps) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [away, setAway] = useState(false);

  // How many replies were unread when the thread was opened (opening reads them, so `bot.unread` drops to 0 at once).
  // The "New" line stays above the first of them until the person sends something or opens another bot.
  const [seen, setSeen] = useState<{ botId: string; unread: number }>({ botId: bot.id, unread: bot.unread });
  const unread = seen.botId === bot.id ? seen.unread : bot.unread;
  useEffect(() => {
    setSeen((was) => (was.botId === bot.id ? was : { botId: bot.id, unread: bot.unread }));
  }, [bot.id, bot.unread]);
  // A reply that arrives while the person reads further up counts too.
  useEffect(() => {
    if (bot.unread > 0 && !pinned.current) setSeen((was) => (was.botId === bot.id ? { botId: bot.id, unread: Math.max(was.unread, bot.unread) } : was));
  }, [bot.id, bot.unread]);
  useEffect(() => {
    if (followToken > 0) setSeen({ botId: bot.id, unread: 0 });
  }, [followToken, bot.id]);

  const unreadId = useMemo(() => firstUnreadId(messages, unread), [messages, unread]);
  const items = useMemo(() => threadItems(messages, { now, unreadId }), [messages, now, unreadId]);

  // Each run of tool steps: the step before it (its previous attempt), and whether it is Fish's latest reply (no bot
  // text after it). Only the latest reply offers Try again or Take control.
  const { before, latest } = useMemo(() => {
    const previous = new Map<string, ToolStep | null>();
    let last: ToolStep | null = null;
    let latestKey: string | null = null;
    for (const item of items) {
      if (item.type === 'activity') {
        previous.set(item.key, last);
        last = item.steps[item.steps.length - 1] ?? null;
        latestKey = item.key;
      } else if (item.type === 'bubble' && item.side === 'bot') {
        latestKey = null;
      }
    }
    return { before: previous, latest: latestKey };
  }, [items]);

  useEffect(() => {
    pinned.current = true;
    setAway(false);
  }, [bot.id, followToken]);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [messages, bot.typing, followToken]);

  const toBottom = () => {
    const el = scroller.current;
    if (!el) return;
    pinned.current = true;
    setAway(false);
    const smooth = typeof el.scrollTo === 'function' && !window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    if (smooth) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
    else el.scrollTop = el.scrollHeight;
  };

  return (
    <div className="ck-potato-body-wrap">
      <div
        className="ck-scroll ck-potato-body"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          const near = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_PX;
          pinned.current = near;
          setAway(!near);
        }}
      >
        <ol className="ck-potato-rows" aria-label={THREAD_COPY.messages}>
          {items.map((item) => {
            switch (item.type) {
              case 'day':
                return <DaySeparator key={item.key} label={item.label} />;
              case 'time':
                return <TimeSeparator key={item.key} label={item.label} />;
              case 'unread':
                return <UnreadDivider key={item.key} />;
              case 'activity':
                return (
                  <ToolActivity
                    key={item.key}
                    steps={item.steps}
                    before={before.get(item.key) ?? null}
                    onSendText={item.key === latest ? onSendText : undefined}
                    onTakeControl={item.key === latest ? onTakeControl : undefined}
                  />
                );
              default:
                return (
                  <MessageRow
                    key={item.message.id}
                    row={item}
                    botId={bot.id}
                    botName={bot.name}
                    agentLabel={agentLabel}
                    transport={transport}
                    onChanged={onChanged}
                    onOpenPane={onOpenPane}
                    paneExists={paneExists}
                    now={now}
                    openLink={openLink}
                    voice={voice}
                  />
                );
            }
          })}
        </ol>
        {bot.typing && <TypingRow name={bot.name} />}
        {messages.length === 0 && !bot.typing && !hideEmpty && <p className="ck-sub ck-potato-empty">{THREAD_COPY.empty(bot.name)}</p>}
      </div>
      {away && (
        <button type="button" className="ck-potato-jump" onClick={toBottom}>
          <ThreadIcon name="arrow-down" size={13} />
          <span>{THREAD_COPY.jump}</span>
        </button>
      )}
    </div>
  );
}
