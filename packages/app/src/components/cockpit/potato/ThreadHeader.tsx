// The top of a bot's thread: face, name, the brain's state word, mood, voice, Stop / Start, and the button that opens
// or closes the bot panel (the monitor icon). Nothing here knows about the message list.
import type { AvatarId } from '../../../lib/companion';
import { brainLabel, brainStateWord, type PotatoBotView, type PotatoSettings, type TalkPersona } from '../../../lib/potato';
import { PANEL_COPY } from '../../../lib/potato-panel';
import { THREAD_COPY } from '../../../lib/potato-thread';
import { Avatar } from '../../companion/Avatar';
import { MoodChip } from './MoodChip';
import { PersonaSwitch } from './PersonaSwitch';
import { runButton } from './model';
import { FloatingButton } from './ui/FloatingButton';
import { Glyph } from './ui/Glyph';
import '../../../styles/cockpit-potato-thread.css';

export interface ThreadHeaderProps {
  bot: PotatoBotView;
  avatar: AvatarId;
  brain: PotatoSettings['brain'];
  /** "Through my agents": no mood, voice or Stop / Start of its own. */
  through: boolean;
  onPersona: (next: TalkPersona) => void;
  onStop: () => void;
  onStart: () => void;
  /** The bot panel: whether it is open, and the button that toggles it. Absent: no button. */
  panel?: { open: boolean; onToggle: () => void };
}

export function ThreadHeader({ bot, avatar, brain, through, onPersona, onStop, onStart, panel }: ThreadHeaderProps) {
  const action = through ? null : runButton(bot.brainState);
  return (
    <header className="ck-potato-head">
      <Avatar id={avatar} size={36} state={bot.typing ? 'working' : 'idle'} name={bot.name} />
      <div className="ck-potato-head-main">
        <h1 className="ck-potato-name">{bot.name}</h1>
        <p className="ck-sub ck-potato-brain">
          {brainLabel(brain)} · {brainStateWord(bot.brainState)}
        </p>
      </div>
      {!through && <MoodChip mood={bot.mood} />}
      {!through && <PersonaSwitch value={bot.talkPersona} onChange={onPersona} />}
      {action === 'stop' && (
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onStop}>
          Stop
        </button>
      )}
      {action === 'start' && (
        <button type="button" className="ck-btn ck-btn--secondary" onClick={onStart}>
          Start
        </button>
      )}
      {panel && (
        <FloatingButton
          className="ck-potato-panel-toggle"
          label={panel.open ? PANEL_COPY.hide : PANEL_COPY.show}
          title={`${panel.open ? PANEL_COPY.hide : PANEL_COPY.show} (${THREAD_COPY.panelShortcut})`}
          pressed={panel.open}
          onClick={panel.onToggle}
        >
          <Glyph name="monitor" />
        </FloatingButton>
      )}
    </header>
  );
}
