// One bot's thread (spec section 4), put together from its parts: the header, the "Needs you" banner, the message list,
// a card when the bot cannot answer, and the message box. Your bubbles are filled, the bot's are outlined. No colour.
// The computer itself lives in the bot panel (BotPanel); the thread only says when the bot needs the person there.
import { useState } from 'react';
import type { AvatarId } from '../../../lib/companion';
import { COPY, plainError, type PotatoBotView, type PotatoMessage, type PotatoSettings, type PotatoTransport, type TalkPersona } from '../../../lib/potato';
import type { ScreenTransport } from '../../../lib/potatoScreen';
import { SessionChatPanel } from '../../chat/SessionChatPanel';
import { BrainStateCard } from './BrainStateCard';
import { Composer } from './Composer';
import { ComputerBanner, bannerDue } from './ComputerBanner';
import { ThreadHeader } from './ThreadHeader';
import { Transcript } from './Transcript';
import { useScreenState } from './useScreenState';
import '../../../styles/cockpit-potato-thread.css';

export { MAX_MESSAGE } from '../../../lib/potato-composer';

/** What the thread needs to know about the bot panel next to it. */
export interface ThreadPanel {
  /** The panel is open. */
  open: boolean;
  /** The panel is open on its Computer tab: the banner has nothing to add. */
  computerShown: boolean;
  onToggle: () => void;
  /** The banner's button: open the panel on the Computer tab. */
  onOpenComputer: () => void;
}

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
  /** The sandbox screen's way to the app: the "Needs you" banner reads it. Absent: no banner. */
  screen?: ScreenTransport;
  /** The bot panel next to the thread. Absent: no panel button and no banner button. */
  panel?: ThreadPanel;
  /** Passed in so a test can pin the clock. */
  now?: number;
}

export function ThreadView(props: ThreadViewProps) {
  const { bot, messages, settings, avatar, agentLabel, transport, onOpenSettings, onNotify, onChanged, companionPaneId, onOpenCompanion, onOpenPane, paneExists, screen, panel, now = Date.now() } = props;
  const [follow, setFollow] = useState(0);
  const through = settings.brain === 'agents' || bot.brainState === 'agents';
  const screenState = useScreenState(through ? undefined : screen);

  const run = async (call: () => Promise<unknown>, failed: string) => {
    try {
      await call();
      onChanged();
    } catch (error) {
      onNotify(plainError(error, failed));
    }
  };

  const head = (
    <ThreadHeader
      bot={bot}
      avatar={avatar}
      brain={settings.brain}
      through={through}
      onPersona={(next: TalkPersona) => void run(() => transport.setPersona(bot.id, next), 'Could not change the voice.')}
      onStop={() => void run(() => transport.stopBot(bot.id), 'Could not stop it.')}
      onStart={() => void run(() => transport.startBot(bot.id), 'Could not start it.')}
      panel={panel && { open: panel.open, onToggle: panel.onToggle }}
    />
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
      {panel && bannerDue(screenState, bot.id, panel.computerShown) && (
        <ComputerBanner botName={bot.name} reason={screenState?.reason ?? null} onOpen={panel.onOpenComputer} />
      )}
      <Transcript
        bot={bot}
        messages={messages}
        agentLabel={agentLabel}
        transport={transport}
        onChanged={onChanged}
        onOpenPane={onOpenPane}
        paneExists={paneExists}
        followToken={follow}
        now={now}
      />

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

      <Composer bot={bot} transport={transport} onChanged={onChanged} onNotify={onNotify} onSent={() => setFollow((n) => n + 1)} />
    </section>
  );
}
