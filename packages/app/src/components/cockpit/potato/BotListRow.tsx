// One row of the bot list: face, name, App tag, time, the last message, a status line (a pill, or the working dots),
// the mood word and an unread dot. Its "..." button and its right-click open the row menu (only when the page gave the
// list actions). In the strip only the face is left, with a dot for a bot that needs you (or a "!" for an error).
import { useEffect, useState } from 'react';
import type { AvatarId } from '../../../lib/companion';
import { moodWord, type PotatoBotView } from '../../../lib/potato';
import { listStatus, listTime, LIST_COPY, needsYou, type ListStatus } from '../../../lib/potato-list';
import { listNextLine, tauriRoutinesTransport, type ListNextLine, type RoutineView, type RoutinesTransport } from '../../../lib/potato-routines';
import { previewLine } from '../../../lib/potato-thread';
import { Avatar } from '../../companion/Avatar';
import { DotSpinner } from '../DotSpinner';
import { StatusPill } from './ui/StatusPill';

export interface BotListRowProps {
  bot: PotatoBotView;
  avatar: AvatarId;
  active: boolean;
  mini: boolean;
  now: number;
  /** The row menu exists (the page gave the list actions). */
  menu: boolean;
  /** This row's menu is open. */
  menuOpen: boolean;
  onOpen: () => void;
  /** Opens the row menu at a point on the screen. */
  onMenu: (x: number, y: number) => void;
  /** Where the bot's routines come from (the desktop app by default). */
  routines?: RoutinesTransport;
}

export function BotListRow({ bot, avatar, active, mini, now, menu, menuOpen, onOpen, onMenu, routines = tauriRoutinesTransport }: BotListRowProps) {
  const status = listStatus(bot);
  const statusWord = status ? LIST_COPY.status[status] : null;
  const [botRoutines, setBotRoutines] = useState<RoutineView[]>([]);
  // The strip shows no second line, so only the full row asks for the routines. A new run (lastAt) asks again.
  useEffect(() => {
    if (mini) return;
    let live = true;
    routines.list(bot.id).then(
      (rows) => live && setBotRoutines(rows),
      () => live && setBotRoutines([]),
    );
    return () => {
      live = false;
    };
  }, [routines, bot.id, bot.lastAt, mini]);
  const nextLine = mini ? null : listNextLine(status, botRoutines, now);
  const classes = ['ck-row', 'ck-potato-conv', active && 'is-active', bot.unread > 0 && 'is-unread', needsYou(status) && 'is-needs'];
  return (
    <div className={`ck-potato-row-wrap${menu ? ' has-menu' : ''}${menuOpen ? ' is-menu-open' : ''}`}>
      <button
        type="button"
        className={classes.filter(Boolean).join(' ')}
        aria-current={active ? 'true' : undefined}
        aria-label={mini ? (statusWord ? `${bot.name}, ${statusWord}` : bot.name) : undefined}
        title={mini ? bot.name : undefined}
        onClick={onOpen}
        onContextMenu={(e) => {
          if (!menu) return;
          e.preventDefault();
          onMenu(e.clientX, e.clientY);
        }}
      >
        <span className="ck-potato-conv-face">
          <Avatar id={avatar} size={26} state={bot.typing ? 'working' : 'idle'} name={bot.name} />
          {mini && <StripMark status={status} unread={bot.unread} />}
        </span>
        {!mini && (
          <span className="ck-potato-conv-main">
            <span className="ck-potato-conv-top">
              <span className="ck-row-title">{bot.name}</span>
              <span className="ck-potato-tag">App</span>
              {bot.lastAt !== null && <span className="ck-potato-time">{listTime(bot.lastAt, now)}</span>}
            </span>
            <span className="ck-potato-conv-bottom">
              <span className="ck-potato-last">{bot.lastLine ? previewLine(bot.lastLine) : 'No messages yet'}</span>
              {bot.unread > 0 && <span className="ck-potato-dot" role="img" aria-label={`${bot.unread} unread`} />}
            </span>
            {(status || nextLine || bot.mood) && (
              <span className="ck-potato-conv-status">
                {nextLine ? <NextLine line={nextLine} /> : status && <StatusLine status={status} />}
                {bot.mood && <span className="ck-potato-mood-word">{moodWord(bot.mood.mood)}</span>}
              </span>
            )}
          </span>
        )}
      </button>
      {menu && !mini && (
        <button
          type="button"
          className="ck-potato-more"
          aria-label={LIST_COPY.more(bot.name)}
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          title={LIST_COPY.more(bot.name)}
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            onMenu(r.right, r.bottom);
          }}
        >
          …
        </button>
      )}
    </div>
  );
}

/** The strip's mark: a dot for "needs you", a ringed "!" for an error, else the unread dot. */
function StripMark({ status, unread }: { status: ListStatus | null; unread: number }) {
  if (status === 'error') return <span className="ck-potato-alert-dot" role="img" aria-label={LIST_COPY.status.error}>!</span>;
  if (needsYou(status)) return <span className="ck-potato-dot ck-potato-dot--corner ck-potato-dot--needs" role="img" aria-label={LIST_COPY.status.needs} />;
  if (unread > 0) return <span className="ck-potato-dot ck-potato-dot--corner" role="img" aria-label={`${unread} unread`} />;
  return null;
}

/** The grey second line under the name: "Waiting for you" with the one accent dot, "Thinking...", "Paused" or "Next: ...". */
export function NextLine({ line }: { line: ListNextLine }) {
  if (line.tone === 'working') {
    return (
      <span className="ck-potato-status ck-potato-status--working">
        <span aria-hidden="true">
          <DotSpinner status="running" size={3} />
        </span>
        {line.text}
      </span>
    );
  }
  return (
    <span className="ck-potato-status">
      {line.tone === 'needs' && <span className="ck-potato-dot" aria-hidden="true" style={{ background: 'var(--accent)' }} />}
      {line.text}
    </span>
  );
}

/** The status line under the preview. Monochrome: weight, a pill, the dots. */
export function StatusLine({ status }: { status: ListStatus }) {
  switch (status) {
    case 'working':
      return (
        <span className="ck-potato-status ck-potato-status--working">
          <span aria-hidden="true">
            <DotSpinner status="running" size={3} />
          </span>
          {LIST_COPY.status.working}
        </span>
      );
    case 'needs':
      return <StatusPill tone="strong">{LIST_COPY.status.needs}</StatusPill>;
    case 'error':
      return (
        <StatusPill tone="quiet">
          <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <circle cx="8" cy="8" r="6.5" />
            <path d="M8 4.5v4.25M8 11.25v.01" />
          </svg>
          {LIST_COPY.status.error}
        </StatusPill>
      );
    case 'paused':
      return <StatusPill tone="dim">{LIST_COPY.status.paused}</StatusPill>;
  }
}
