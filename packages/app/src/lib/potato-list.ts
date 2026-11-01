// The bot list on the left of the Potato Bot page: its words and its rules. Which status a bot shows, which section it
// sits in, what the search finds, how a time reads, what the Activity list says and which menu items a bot gets are all
// decided here, as plain functions, so the tests need no page. The components only draw what these return.
import { errorSentence, type BrainState, type PotatoBotView } from './potato';
import { previewLine } from './potato-thread';

export const LIST_COPY = {
  resize: 'Resize the bot list',
  hide: 'Make the bot list smaller',
  show: 'Show the bot list',
  viewLabel: 'Bots or activity',
  bots: 'Bots',
  activity: 'Activity',
  search: 'Search bots',
  searchPlaceholder: 'Search',
  clearSearch: 'Clear search',
  noMatch: (query: string) => `No bots match "${query}".`,
  noActivity: 'Nothing has happened yet.',
  needsYou: 'Needs you',
  recent: 'Recent',
  more: (name: string) => `Actions for ${name}`,
  menu: 'Bot actions',
  pause: 'Pause',
  resume: 'Resume',
  markRead: 'Mark read',
  status: { working: 'Working', needs: 'Needs you', error: 'Error', paused: 'Paused' },
  sentence: {
    needsAnswer: 'Needs your answer.',
    needsKey: 'Needs a model key.',
    needsNode: 'Needs Node.js.',
    working: 'Working on a reply.',
    paused: 'Paused.',
    limit: "Has used today's limit.",
    missing: 'Brain is missing from this install.',
    failed: 'Hit a problem.',
    lastLine: (line: string) => `Last message: ${line}`,
  },
};

// ---- keys ---------------------------------------------------------------------------------------

/** Ctrl/Cmd+K: focus the search box (only while the Potato page is on screen; the page mounts this list). */
export function isSearchShortcut(e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>): boolean {
  return (e.ctrlKey || e.metaKey) && !(e.ctrlKey && e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k';
}

// ---- status -------------------------------------------------------------------------------------

/** What a row says about a bot. Error wins over needs-you, which wins over working, which wins over paused. */
export type ListStatus = 'error' | 'needs' | 'working' | 'paused';

type StatusInput = Pick<PotatoBotView, 'name' | 'brainState' | 'lastError' | 'typing' | 'openCards'>;

export function listStatus(bot: StatusInput): ListStatus | null {
  if (bot.brainState === 'missing_program' || bot.brainState === 'daily_limit' || errorSentence(bot.name, bot.lastError) !== null) return 'error';
  if (bot.openCards > 0 || bot.brainState === 'needs_key' || bot.brainState === 'needs_node') return 'needs';
  if (bot.typing) return 'working';
  if (bot.brainState === 'stopped') return 'paused';
  return null;
}

/** A bot that waits for the person: pinned in its own section, and the strip shows its dot. */
export function needsYou(status: ListStatus | null): boolean {
  return status === 'needs' || status === 'error';
}

/** The one short sentence for a bot's status, or null when it has none. */
export function statusSentence(bot: StatusInput): string | null {
  switch (listStatus(bot)) {
    case 'error':
      if (bot.brainState === 'missing_program') return LIST_COPY.sentence.missing;
      if (bot.brainState === 'daily_limit') return LIST_COPY.sentence.limit;
      return errorSentence(bot.name, bot.lastError) ?? LIST_COPY.sentence.failed;
    case 'needs':
      if (bot.openCards > 0) return LIST_COPY.sentence.needsAnswer;
      return bot.brainState === 'needs_node' ? LIST_COPY.sentence.needsNode : LIST_COPY.sentence.needsKey;
    case 'working':
      return LIST_COPY.sentence.working;
    case 'paused':
      return LIST_COPY.sentence.paused;
    default:
      return null;
  }
}

// ---- search and sections ------------------------------------------------------------------------

/** Name or last message, without case. An empty search finds every bot. */
export function matchesSearch(bot: Pick<PotatoBotView, 'name' | 'lastLine'>, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return bot.name.toLowerCase().includes(q) || previewLine(bot.lastLine ?? '').toLowerCase().includes(q);
}

/** Newest first. A bot with no message yet goes last. Ties keep the name order. */
export function byRecent(a: Pick<PotatoBotView, 'name' | 'lastAt'>, b: Pick<PotatoBotView, 'name' | 'lastAt'>): number {
  const diff = (b.lastAt ?? 0) - (a.lastAt ?? 0);
  return diff !== 0 ? diff : a.name.localeCompare(b.name);
}

export interface ListSections<T> {
  /** Pinned on top. */
  needs: T[];
  /** Everything else, by recent. */
  rest: T[];
}

/** The rows the list shows: the search applied, split into "Needs you" and the rest, both by recent. */
export function listSections<T extends PotatoBotView>(bots: readonly T[], query = ''): ListSections<T> {
  const needs: T[] = [];
  const rest: T[] = [];
  for (const bot of bots.filter((b) => matchesSearch(b, query)).sort(byRecent)) {
    (needsYou(listStatus(bot)) ? needs : rest).push(bot);
  }
  return { needs, rest };
}

/** The strip's order: the same as the list's, with no search. */
export function stripOrder<T extends PotatoBotView>(bots: readonly T[]): T[] {
  const { needs, rest } = listSections(bots);
  return [...needs, ...rest];
}

// ---- time ---------------------------------------------------------------------------------------

const DAY_MS = 86_400_000;
const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** `3:10 AM`: the clock, with a plain space before AM or PM. */
export function clockLabel(at: number): string {
  const d = new Date(at);
  const h = d.getHours();
  return `${h % 12 || 12}:${String(d.getMinutes()).padStart(2, '0')} ${h < 12 ? 'AM' : 'PM'}`;
}

/** The time on a row, like the hero picture: the clock today, `Yesterday`, a weekday within the week, then `Oct 28`. */
export function listTime(at: number, now: number): string {
  const when = new Date(at);
  const today = new Date(now);
  const dayOf = (d: Date) => Math.round(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / DAY_MS);
  const days = dayOf(today) - dayOf(when);
  if (days <= 0) return clockLabel(at);
  if (days === 1) return 'Yesterday';
  if (days < 7) return WEEKDAYS[when.getDay()];
  const year = when.getFullYear() === today.getFullYear() ? '' : `, ${when.getFullYear()}`;
  return `${MONTHS[when.getMonth()]} ${when.getDate()}${year}`;
}

// ---- activity -----------------------------------------------------------------------------------

export interface ActivityEvent {
  botId: string;
  botName: string;
  text: string;
  /** The time the bot last spoke (events carry no time of their own). Null: none yet. */
  at: number | null;
}

/**
 * Recent things across the bots, newest first: each bot's status sentence and its last message. Built only from what
 * the page already has (the bot rows); nothing is stored for it.
 */
export function activityEvents<T extends PotatoBotView>(bots: readonly T[], query = ''): ActivityEvent[] {
  const events: ActivityEvent[] = [];
  for (const bot of bots) {
    if (!matchesSearch(bot, query)) continue;
    const sentence = statusSentence(bot);
    if (sentence) events.push({ botId: bot.id, botName: bot.name, text: sentence, at: bot.lastAt });
    if (bot.lastLine && bot.lastAt !== null) {
      events.push({ botId: bot.id, botName: bot.name, text: LIST_COPY.sentence.lastLine(previewLine(bot.lastLine)), at: bot.lastAt });
    }
  }
  return events.sort((a, b) => (b.at ?? -1) - (a.at ?? -1));
}

// ---- the row menu -------------------------------------------------------------------------------

export type BotMenuAction = 'pause' | 'resume' | 'markRead';

export interface BotMenuItem {
  action: BotMenuAction;
  label: string;
  disabled: boolean;
}

const RUNNING: readonly BrainState[] = ['ready', 'starting', 'restarting'];
const HALTED: readonly BrainState[] = ['idle', 'stopped'];

/**
 * The menu for one bot: Pause while it runs or Resume when it is stopped, and Mark read (greyed when nothing is
 * unread). Only the actions the page gave the list appear. Rename and Delete are not here: the app has no command for
 * them yet.
 */
export function botMenuItems(bot: Pick<PotatoBotView, 'brainState' | 'unread'>, available: readonly BotMenuAction[]): BotMenuItem[] {
  const items: BotMenuItem[] = [];
  if (RUNNING.includes(bot.brainState) && available.includes('pause')) items.push({ action: 'pause', label: LIST_COPY.pause, disabled: false });
  if (HALTED.includes(bot.brainState) && available.includes('resume')) items.push({ action: 'resume', label: LIST_COPY.resume, disabled: false });
  if (available.includes('markRead')) items.push({ action: 'markRead', label: LIST_COPY.markRead, disabled: bot.unread === 0 });
  return items;
}
