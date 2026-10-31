// The 240 px list on the left of the Potato Bot page (spec section 4). One group per bot; in phase 1 a group holds
// one thread, the "App" one. Each row: face, name, last line, time, an unread dot and the mood word. "New bot" opens
// the companion sheet; a companion that is not a bot yet can be turned into one with one click.
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { COPY, formatWhen, moodWord, type PotatoBotView } from '../../../lib/potato';
import { Avatar } from '../../companion/Avatar';
import { IconPlus } from '../Icons';

export interface ConversationListProps {
  bots: PotatoBotView[];
  /** The face of each bot (it is a companion). A bot without a face gets the first one. */
  avatars: ReadonlyMap<string, AvatarId>;
  openId: string | null;
  onOpen: (botId: string) => void;
  onNew: () => void;
  /** Companions that are not bots yet. */
  candidates: CompanionView[];
  onUseCompanion: (companionId: string) => void;
  /** Passed in so a test can pin the clock. */
  now?: number;
}

export function ConversationList({ bots, avatars, openId, onOpen, onNew, candidates, onUseCompanion, now = Date.now() }: ConversationListProps) {
  return (
    <nav className="ck-potato-list" aria-label="Conversations">
      <div className="ck-potato-list-scroll">
        {bots.length === 0 && <p className="ck-sub ck-potato-empty">{COPY.emptyList}</p>}
        {bots.map((bot) => {
          const active = bot.id === openId;
          return (
            <ul className="ck-potato-group" key={bot.id} aria-label={bot.name}>
              <li>
                <button
                  type="button"
                  className={`ck-row ck-potato-conv${active ? ' is-active' : ''}${bot.unread > 0 ? ' is-unread' : ''}`}
                  aria-current={active ? 'true' : undefined}
                  onClick={() => onOpen(bot.id)}
                >
                  <span className="ck-potato-conv-face">
                    <Avatar id={avatars.get(bot.id) ?? 'mochi'} size={26} state={bot.typing ? 'working' : 'idle'} name={bot.name} />
                  </span>
                  <span className="ck-potato-conv-main">
                    <span className="ck-potato-conv-top">
                      <span className="ck-row-title">{bot.name}</span>
                      <span className="ck-potato-tag">App</span>
                      {bot.lastAt !== null && <span className="ck-potato-time">{formatWhen(bot.lastAt, now)}</span>}
                    </span>
                    <span className="ck-potato-conv-bottom">
                      <span className="ck-potato-last">{bot.lastLine ?? 'No messages yet'}</span>
                      {bot.mood && <span className="ck-potato-mood-word">{moodWord(bot.mood.mood)}</span>}
                      {bot.unread > 0 && <span className="ck-potato-dot" role="img" aria-label={`${bot.unread} unread`} />}
                    </span>
                  </span>
                </button>
              </li>
            </ul>
          );
        })}
      </div>
      <div className="ck-potato-list-foot">
        {candidates.map((companion) => (
          <button type="button" key={companion.id} className="ck-row ck-potato-make" onClick={() => onUseCompanion(companion.id)}>
            <span className="ck-row-glyph">
              <IconPlus />
            </span>
            <span className="ck-row-main">
              <span className="ck-row-title">Make {companion.name} a bot</span>
            </span>
          </button>
        ))}
        <button type="button" className="ck-row ck-potato-make" onClick={onNew}>
          <span className="ck-row-glyph">
            <IconPlus />
          </span>
          <span className="ck-row-main">
            <span className="ck-row-title">New bot</span>
          </span>
        </button>
      </div>
    </nav>
  );
}
