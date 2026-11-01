// The bot list on the left of the Potato Bot page (spec section 4). One group per bot; in phase 1 a group holds one
// thread, the "App" one. Each row: face, name, last line, time, an unread dot and the mood word. "New bot" opens the
// companion sheet; a companion that is not a bot yet can be turned into one with one click. Its width comes from the
// page (the `--potato-list-w` variable); in the strip mode (`mini`) only the faces are left.
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { COPY, formatWhen, moodWord, type PotatoBotView } from '../../../lib/potato';
import { LIST_COPY } from '../../../lib/potato-list';
import { ARTIFACT_COPY } from '../../../lib/potato-artifacts';
import { Avatar } from '../../companion/Avatar';
import { IconPlus } from '../Icons';
import { Glyph } from './ui/Glyph';
import '../../../styles/cockpit-potato-list.css';

export interface BotListProps {
  bots: PotatoBotView[];
  /** The face of each bot (it is a companion). A bot without a face gets the first one. */
  avatars: ReadonlyMap<string, AvatarId>;
  openId: string | null;
  onOpen: (botId: string) => void;
  onNew: () => void;
  /** Companions that are not bots yet. */
  candidates: CompanionView[];
  onUseCompanion: (companionId: string) => void;
  /** The strip: faces only. */
  mini?: boolean;
  /** Makes the list a strip, or opens it again. Absent: no button. */
  onToggleMini?: () => void;
  /** Opens the Artifacts page (its button sits in the list's header). Absent: no button. */
  onArtifacts?: () => void;
  artifactsOpen?: boolean;
  /** Passed in so a test can pin the clock. */
  now?: number;
}

export function BotList({
  bots,
  avatars,
  openId,
  onOpen,
  onNew,
  candidates,
  onUseCompanion,
  mini = false,
  onToggleMini,
  onArtifacts,
  artifactsOpen = false,
  now = Date.now(),
}: BotListProps) {
  return (
    <nav className={`ck-potato-list${mini ? ' is-mini' : ''}`} aria-label="Conversations">
      {onToggleMini && (
        <div className="ck-potato-list-head">
          {!mini && <span className="ck-potato-list-title">{LIST_COPY.title}</span>}
          {onArtifacts && (
            <button
              type="button"
              className={`ck-icon-btn ck-potato-artifacts-open${artifactsOpen ? ' is-on' : ''}`}
              aria-label={ARTIFACT_COPY.title}
              aria-pressed={artifactsOpen}
              title={ARTIFACT_COPY.title}
              onClick={onArtifacts}
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M5.5 4.5L2.5 8l3 3.5M10.5 4.5l3 3.5-3 3.5M9 3l-2 10" />
              </svg>
            </button>
          )}
          <button
            type="button"
            className="ck-icon-btn"
            aria-label={mini ? LIST_COPY.show : LIST_COPY.hide}
            title={`${mini ? LIST_COPY.show : LIST_COPY.hide} (Ctrl+B)`}
            onClick={onToggleMini}
          >
            <Glyph name="list" />
          </button>
        </div>
      )}
      <div className="ck-potato-list-scroll">
        {bots.length === 0 && !mini && <p className="ck-sub ck-potato-empty">{COPY.emptyList}</p>}
        {bots.map((bot) => {
          const active = bot.id === openId;
          return (
            <ul className="ck-potato-group" key={bot.id} aria-label={bot.name}>
              <li>
                <button
                  type="button"
                  className={`ck-row ck-potato-conv${active ? ' is-active' : ''}${bot.unread > 0 ? ' is-unread' : ''}`}
                  aria-current={active ? 'true' : undefined}
                  aria-label={mini ? bot.name : undefined}
                  title={mini ? bot.name : undefined}
                  onClick={() => onOpen(bot.id)}
                >
                  <span className="ck-potato-conv-face">
                    <Avatar id={avatars.get(bot.id) ?? 'mochi'} size={26} state={bot.typing ? 'working' : 'idle'} name={bot.name} />
                    {mini && bot.unread > 0 && <span className="ck-potato-dot ck-potato-dot--corner" role="img" aria-label={`${bot.unread} unread`} />}
                  </span>
                  {!mini && (
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
                  )}
                </button>
              </li>
            </ul>
          );
        })}
      </div>
      <div className="ck-potato-list-foot">
        {candidates.map((companion) => (
          <button
            type="button"
            key={companion.id}
            className="ck-row ck-potato-make"
            aria-label={mini ? `Make ${companion.name} a bot` : undefined}
            title={mini ? `Make ${companion.name} a bot` : undefined}
            onClick={() => onUseCompanion(companion.id)}
          >
            <span className="ck-row-glyph">
              <IconPlus />
            </span>
            {!mini && (
              <span className="ck-row-main">
                <span className="ck-row-title">Make {companion.name} a bot</span>
              </span>
            )}
          </button>
        ))}
        <button type="button" className="ck-row ck-potato-make" aria-label={mini ? 'New bot' : undefined} title={mini ? 'New bot' : undefined} onClick={onNew}>
          <span className="ck-row-glyph">
            <IconPlus />
          </span>
          {!mini && (
            <span className="ck-row-main">
              <span className="ck-row-title">New bot</span>
            </span>
          )}
        </button>
      </div>
    </nav>
  );
}
