// One bot's thread (spec section 4): the header (face, name, mood, voice, brain, Stop/Start), the bubbles, the typing
// row, a card when the bot cannot answer, and the message box. Your bubbles are filled, the bot's are outlined.
// Enter sends; Shift+Enter adds a line. No colour.
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import type { AvatarId } from '../../../lib/companion';
import {
  brainLabel,
  brainStateWord,
  COPY,
  formatWhen,
  plainError,
  type PotatoBotView,
  type PotatoMessage,
  type PotatoSettings,
  type PotatoTransport,
  type TalkPersona,
} from '../../../lib/potato';
import { Avatar } from '../../companion/Avatar';
import { SessionChatPanel } from '../../chat/SessionChatPanel';
import { BrainStateCard } from './BrainStateCard';
import { HelperCard } from './HelperCard';
import { MoodChip } from './MoodChip';
import { PersonaSwitch } from './PersonaSwitch';
import { TypingRow } from './TypingRow';
import { canWrite, runButton, threadRows } from './model';

/** The longest message the brain takes (spec section 2). */
export const MAX_MESSAGE = 4000;
/** Closer than this to the bottom, a new line keeps the thread pinned to it. */
const STICK_PX = 120;

export interface ThreadViewProps {
  bot: PotatoBotView;
  messages: PotatoMessage[];
  settings: PotatoSettings;
  avatar: AvatarId;
  /** The label of the agent that does a helper's work (the companion's agent). */
  agentLabel: string;
  transport: PotatoTransport;
  onOpenSettings: () => void;
  /** Said when a command fails and no card is there to say it. */
  onNotify: (text: string) => void;
  /** Called after a command, so the state is read again at once. */
  onChanged: () => void;
  /** The "Through my agents" brain has no thread of its own here: the thread is the companion's own chat, in its pane. */
  companionPaneId?: string | null;
  /** The companion's own page (the "Through my agents" line under the header). */
  onOpenCompanion?: (id: string) => void;
  /** Opens a helper's pane (the Open button of a finished helper). Absent: no button. */
  onOpenPane?: (paneId: string) => void;
  /** Whether that pane is still there; the button is shown only then. Default: yes. */
  paneExists?: (paneId: string) => boolean;
  /** Passed in so a test can pin the clock. */
  now?: number;
}

export function ThreadView(props: ThreadViewProps) {
  const { bot, messages, settings, avatar, agentLabel, transport, onOpenSettings, onNotify, onChanged, companionPaneId, onOpenCompanion, onOpenPane, paneExists, now = Date.now() } = props;
  const [draft, setDraft] = useState('');
  const [sending, setSending] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const pinned = useRef(true);
  const through = settings.brain === 'agents' || bot.brainState === 'agents';

  const run = async (call: () => Promise<unknown>, failed: string) => {
    try {
      await call();
      onChanged();
    } catch (error) {
      onNotify(plainError(error, failed));
    }
  };

  // Follow the bottom while the person is at the bottom; reading further up is left alone.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [messages, bot.typing]);
  useEffect(() => {
    pinned.current = true;
  }, [bot.id]);
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
    pinned.current = true;
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

  const stop = through ? null : runButton(bot.brainState);
  const head = (
    <header className="ck-potato-head">
      <Avatar id={avatar} size={36} state={bot.typing ? 'working' : 'idle'} name={bot.name} />
      <div className="ck-potato-head-main">
        <h1 className="ck-potato-name">{bot.name}</h1>
        <p className="ck-sub ck-potato-brain">
          {brainLabel(settings.brain)} · {brainStateWord(bot.brainState)}
        </p>
      </div>
      {!through && <MoodChip mood={bot.mood} />}
      {!through && <PersonaSwitch value={bot.talkPersona} onChange={(next: TalkPersona) => void run(() => transport.setPersona(bot.id, next), 'Could not change the voice.')} />}
      {stop === 'stop' && (
        <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void run(() => transport.stopBot(bot.id), 'Could not stop it.')}>
          Stop
        </button>
      )}
      {stop === 'start' && (
        <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void run(() => transport.startBot(bot.id), 'Could not start it.')}>
          Start
        </button>
      )}
    </header>
  );

  if (through) {
    return (
      <section className="ck-potato-thread" aria-label={`${bot.name}, through your agents`}>
        {head}
        <p className="ck-sub ck-potato-through-note">{COPY.agentsOnly}</p>
        {companionPaneId ? (
          // The companion's own chat: a new pane with the same id (a restart) needs a new chat.
          <div className="ck-scroll ck-fade-in ck-potato-chat" key={companionPaneId}>
            <SessionChatPanel paneId={companionPaneId} reply={{ blockedReason: null }} />
          </div>
        ) : (
          <div className="ck-potato-through">
            <p className="ck-sub">{bot.name} is not awake. Open its page to wake it up.</p>
            {onOpenCompanion && (
              <button type="button" className="ck-btn ck-btn--secondary" onClick={() => onOpenCompanion(bot.id)}>
                Open {bot.name}
              </button>
            )}
          </div>
        )}
      </section>
    );
  }

  return (
    <section className="ck-potato-thread" aria-label={`Chat with ${bot.name}`}>
      {head}
      <div
        className="ck-scroll ck-potato-body"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_PX;
        }}
      >
        <ol className="ck-potato-rows" aria-label="Messages">
          {threadRows(messages).map((row) => {
            const m = row.message;
            if (row.type === 'note') {
              const canOpen = m.kind === 'helper_result' && !!m.paneId && !!onOpenPane && (paneExists?.(m.paneId) ?? true);
              return (
                <li key={m.id} className="ck-potato-note" role="note">
                  {m.text}
                  {canOpen && (
                    <>
                      {' '}
                      <button type="button" className="ck-potato-note-open" onClick={() => onOpenPane?.(m.paneId!)}>
                        Open
                      </button>
                    </>
                  )}
                </li>
              );
            }
            if (row.type === 'card') {
              return (
                <li key={m.id} className="ck-potato-card-row">
                  <HelperCard
                    name={bot.name}
                    task={m.text}
                    project={m.project}
                    agentLabel={agentLabel}
                    open={row.open}
                    onStart={async (folder) => {
                      await transport.helperStart(bot.id, m.requestId ?? '', folder);
                      onChanged();
                    }}
                    onNotNow={async () => {
                      await transport.helperDecline(bot.id, m.requestId ?? '');
                      onChanged();
                    }}
                    loadFolders={() => transport.helperFolders(bot.id)}
                  />
                </li>
              );
            }
            return (
              <li key={m.id} className={`ck-potato-line ck-potato-line--${row.side}${row.first ? ' is-first' : ''}`}>
                <div className={`ck-potato-bubble ck-potato-bubble--${row.side}`} title={formatWhen(m.at, now)}>
                  {m.text}
                </div>
              </li>
            );
          })}
        </ol>
        {bot.typing && <TypingRow name={bot.name} />}
      </div>

      <BrainStateCard
        name={bot.name}
        state={bot.brainState}
        lastError={bot.lastError}
        limitCents={bot.limitCents}
        onStart={async () => {
          await transport.startBot(bot.id);
          onChanged();
        }}
        onInstallNode={async () => {
          await transport.installNode();
          onChanged();
        }}
        onOpenSettings={onOpenSettings}
      />

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
    </section>
  );
}
