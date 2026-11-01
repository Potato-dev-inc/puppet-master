// The bot list on the left of the Potato Bot page (spec section 4). Top: the Bots | Activity switch, a search box and the
// button that makes the list a strip. Bots: "Needs you" pinned on top, the rest by recent; each row has its face, name,
// time, last message, a status line and the mood word. Activity: recent things across the bots. The row menu (Pause or
// Resume, Mark read) appears only when the page gives the list its actions. "New bot" opens the companion sheet; a
// companion that is not a bot yet can be turned into one with one click. Its width comes from the page (the
// `--potato-list-w` variable); in the strip mode (`mini`) only the faces are left, with a dot for a bot that needs you.
import { useEffect, useMemo, useRef, useState } from 'react';
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { COPY, type PotatoBotView } from '../../../lib/potato';
import { activityEvents, botMenuItems, isSearchShortcut, listSections, LIST_COPY, type BotMenuAction } from '../../../lib/potato-list';
import { IconPlus } from '../Icons';
import { ActivityList } from './ActivityList';
import { BotContextMenu } from './BotContextMenu';
import { BotListRow } from './BotListRow';
import { BotListSearch } from './BotListSearch';
import { Glyph } from './ui/Glyph';
import '../../../styles/cockpit-potato-list.css';

/** What the row menu can do. Only the ones the page passes appear in the menu. */
export interface BotListActions {
  pause?: (botId: string) => void;
  resume?: (botId: string) => void;
  markRead?: (botId: string) => void;
}

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
  /** The row menu's actions. Absent (or empty): no "..." button and no right-click menu. */
  actions?: BotListActions;
  /** Passed in so a test can pin the clock. */
  now?: number;
}

type View = 'bots' | 'activity';
const VIEWS: { id: View; label: string }[] = [
  { id: 'bots', label: LIST_COPY.bots },
  { id: 'activity', label: LIST_COPY.activity },
];
const ALL_ACTIONS: readonly BotMenuAction[] = ['pause', 'resume', 'markRead'];

export function BotList({ bots, avatars, openId, onOpen, onNew, candidates, onUseCompanion, mini = false, onToggleMini, actions, now = Date.now() }: BotListProps) {
  const [view, setView] = useState<View>('bots');
  const [query, setQuery] = useState('');
  const [menu, setMenu] = useState<{ botId: string; x: number; y: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const focusWhenShown = useRef(false);

  const available = ALL_ACTIONS.filter((action) => typeof actions?.[action] === 'function');
  const menuOn = available.length > 0;
  // The strip never filters: it shows every bot, the ones that need you first.
  const sections = useMemo(() => listSections(bots, mini ? '' : query), [bots, query, mini]);
  const events = useMemo(() => (view === 'activity' && !mini ? activityEvents(bots, query) : []), [bots, query, view, mini]);
  const menuBot = menu ? bots.find((b) => b.id === menu.botId) : undefined;
  const menuItems = menuBot ? botMenuItems(menuBot, available) : [];

  // Ctrl/Cmd+K: focus the search. This list exists only while the Potato page is on screen. In the strip it opens the
  // list first and focuses the search once it is drawn.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isSearchShortcut(e)) return;
      e.preventDefault();
      if (mini) {
        if (!onToggleMini) return;
        focusWhenShown.current = true;
        onToggleMini();
        return;
      }
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [mini, onToggleMini]);

  useEffect(() => {
    if (!mini && focusWhenShown.current) {
      focusWhenShown.current = false;
      inputRef.current?.focus();
    }
  }, [mini]);

  const renderRow = (bot: PotatoBotView) => (
    <li key={bot.id}>
      <BotListRow
        bot={bot}
        avatar={avatars.get(bot.id) ?? 'mochi'}
        active={bot.id === openId}
        mini={mini}
        now={now}
        menu={menuOn}
        menuOpen={menu?.botId === bot.id}
        onOpen={() => onOpen(bot.id)}
        onMenu={(x, y) => setMenu({ botId: bot.id, x, y })}
      />
    </li>
  );

  const section = (title: string | undefined, rows: PotatoBotView[]) => (
    <section className="ck-potato-section" key={title ?? 'all'}>
      {title && <h2 className="ck-potato-section-title">{title}</h2>}
      <ul className="ck-potato-group" aria-label={title ?? LIST_COPY.bots}>
        {rows.map(renderRow)}
      </ul>
    </section>
  );

  const noRows = mini ? sections.needs.length + sections.rest.length === 0 : view === 'bots' && sections.needs.length + sections.rest.length === 0;

  return (
    <nav className={`ck-potato-list${mini ? ' is-mini' : ''}`} aria-label="Conversations">
      {(!mini || onToggleMini) && (
        <div className="ck-potato-list-head">
          {!mini && (
            <div className="ck-seg ck-seg--sm ck-potato-views" role="group" aria-label={LIST_COPY.viewLabel}>
              {VIEWS.map((option) => (
                <button key={option.id} type="button" className="ck-seg-btn" aria-pressed={view === option.id} onClick={() => setView(option.id)}>
                  {option.label}
                </button>
              ))}
            </div>
          )}
          {onToggleMini && (
            <button
              type="button"
              className="ck-icon-btn ck-potato-list-toggle"
              aria-label={mini ? LIST_COPY.show : LIST_COPY.hide}
              title={`${mini ? LIST_COPY.show : LIST_COPY.hide} (Ctrl+B)`}
              onClick={onToggleMini}
            >
              <Glyph name="list" />
            </button>
          )}
        </div>
      )}
      {!mini && <BotListSearch value={query} onChange={setQuery} inputRef={inputRef} />}
      <div className="ck-potato-list-scroll">
        {bots.length === 0 && !mini && <p className="ck-sub ck-potato-empty">{COPY.emptyList}</p>}
        {bots.length > 0 && noRows && view === 'bots' && <p className="ck-sub ck-potato-empty">{LIST_COPY.noMatch(query.trim())}</p>}
        {mini && <ul className="ck-potato-group">{[...sections.needs, ...sections.rest].map(renderRow)}</ul>}
        {!mini && view === 'bots' && (
          <>
            {sections.needs.length > 0 && section(LIST_COPY.needsYou, sections.needs)}
            {sections.rest.length > 0 && section(sections.needs.length > 0 ? LIST_COPY.recent : undefined, sections.rest)}
          </>
        )}
        {!mini && view === 'activity' && <ActivityList events={events} now={now} onOpen={onOpen} />}
      </div>
      {menu && menuBot && menuItems.length > 0 && (
        <BotContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onPick={(action) => {
            setMenu(null);
            actions?.[action]?.(menuBot.id);
          }}
          onClose={() => setMenu(null)}
        />
      )}
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
