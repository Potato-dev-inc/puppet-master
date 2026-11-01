// The bot list's rules: status, sections, search, time labels, the Activity events, the row menu and the Ctrl/Cmd+K key.
import { describe, expect, it } from 'vitest';
import type { PotatoBotView } from './potato';
import { activityEvents, botMenuItems, byRecent, clockLabel, isSearchShortcut, listSections, listStatus, listTime, matchesSearch, statusSentence, stripOrder } from './potato-list';

const bot = (over: Partial<PotatoBotView> = {}): PotatoBotView => ({
  id: 'fish',
  name: 'Fish',
  talkPersona: 'fish',
  brainState: 'ready',
  lastError: null,
  typing: false,
  mood: null,
  provider: null,
  unread: 0,
  lastLine: null,
  lastAt: null,
  usedTodayNano: 0,
  limitCents: 50,
  openCards: 0,
  ...over,
});

// Sunday 1 November 2026, 15:00 on the clock of the machine running the test.
const NOW = new Date(2026, 10, 1, 15, 0).getTime();
const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m, d, h, min).getTime();

describe('listTime', () => {
  it('shows the clock for a time today, with AM or PM and a plain space', () => {
    expect(listTime(at(2026, 10, 1, 15, 10), NOW)).toBe('3:10 PM');
    expect(listTime(at(2026, 10, 1, 3, 10), NOW)).toBe('3:10 AM');
    expect(listTime(at(2026, 10, 1, 0, 5), NOW)).toBe('12:05 AM');
    expect(clockLabel(at(2026, 10, 1, 12, 30))).toBe('12:30 PM');
  });

  it('says Yesterday for the day before', () => {
    expect(listTime(at(2026, 9, 31, 22, 0), NOW)).toBe('Yesterday');
  });

  it('says the weekday within the last six days, like the hero picture', () => {
    // 26 October 2026 is a Monday.
    expect(listTime(at(2026, 9, 26, 10, 0), NOW)).toBe('Monday');
  });

  it('shows the short date from a week back, with the year only when it is another year', () => {
    expect(listTime(at(2026, 9, 3, 8, 0), NOW)).toBe('Oct 3');
    expect(listTime(at(2025, 11, 30, 8, 0), NOW)).toBe('Dec 30, 2025');
  });
});

describe('listStatus and statusSentence', () => {
  it('reads Working from typing, Needs you from an open card, Paused from a stop', () => {
    expect(listStatus(bot({ typing: true }))).toBe('working');
    expect(listStatus(bot({ openCards: 1 }))).toBe('needs');
    expect(listStatus(bot({ brainState: 'needs_key' }))).toBe('needs');
    expect(listStatus(bot({ brainState: 'stopped' }))).toBe('paused');
    expect(listStatus(bot())).toBeNull();
  });

  it('reads Error from a known brain error or a brain that is missing, and error wins over needs you', () => {
    expect(listStatus(bot({ lastError: 'PROVIDER_AUTH' }))).toBe('error');
    expect(listStatus(bot({ brainState: 'missing_program' }))).toBe('error');
    expect(listStatus(bot({ openCards: 1, lastError: 'PROVIDER_AUTH' }))).toBe('error');
    // An error code with no sentence is not worth a status.
    expect(listStatus(bot({ lastError: 'SOMETHING_NEW' }))).toBeNull();
  });

  it('gives one short sentence for the status', () => {
    expect(statusSentence(bot({ openCards: 2 }))).toBe('Needs your answer.');
    expect(statusSentence(bot({ brainState: 'needs_node' }))).toBe('Needs Node.js.');
    expect(statusSentence(bot({ typing: true }))).toBe('Working on a reply.');
    expect(statusSentence(bot())).toBeNull();
  });
});

describe('listSections', () => {
  const bots = [
    bot({ id: 'old-needs', name: 'Old', openCards: 1, lastAt: at(2026, 10, 1, 8, 0) }),
    bot({ id: 'newest', name: 'Newest', lastAt: at(2026, 10, 1, 14, 0) }),
    bot({ id: 'middle', name: 'Middle', lastAt: at(2026, 9, 30, 9, 0) }),
    bot({ id: 'new-needs', name: 'Newer needs', lastAt: at(2026, 9, 30, 9, 0), brainState: 'missing_program' }),
    bot({ id: 'silent', name: 'Silent', lastAt: null }),
  ];

  it('pins the bots that need you on top, then the rest, both by recent', () => {
    const { needs, rest } = listSections(bots);
    expect(needs.map((b) => b.name)).toEqual(['Old', 'Newer needs']);
    expect(rest.map((b) => b.name)).toEqual(['Newest', 'Middle', 'Silent']);
  });

  it('sends a bot with no message yet to the bottom of its section', () => {
    expect([...bots].sort(byRecent).map((b) => b.name)).toEqual(['Newest', 'Old', 'Middle', 'Newer needs', 'Silent']);
  });

  it('the strip has the same order, with no search', () => {
    expect(stripOrder(bots).map((b) => b.id)).toEqual(['old-needs', 'new-needs', 'newest', 'middle', 'silent']);
  });

  it('applies the search to both sections', () => {
    const { needs, rest } = listSections(bots, 'new');
    expect(needs.map((b) => b.name)).toEqual(['Newer needs']);
    expect(rest.map((b) => b.name)).toEqual(['Newest']);
  });
});

describe('matchesSearch', () => {
  it('finds a bot by name or by its last message, without case', () => {
    const fish = bot({ name: 'Fish', lastLine: 'Say Hello to Ana' });
    expect(matchesSearch(fish, 'fish')).toBe(true);
    expect(matchesSearch(fish, 'HELLO')).toBe(true);
    expect(matchesSearch(fish, '  ana ')).toBe(true);
    expect(matchesSearch(fish, 'bee')).toBe(false);
  });

  it('finds every bot with an empty search, and a bot with no message by its name only', () => {
    expect(matchesSearch(bot(), '')).toBe(true);
    expect(matchesSearch(bot({ lastLine: null }), 'message')).toBe(false);
  });
});

describe('activityEvents', () => {
  const bots = [
    bot({ id: 'a', name: 'Ada', openCards: 1, lastLine: 'Draft is ready', lastAt: at(2026, 10, 1, 10, 0) }),
    bot({ id: 'b', name: 'Bo', lastLine: 'Hi there', lastAt: at(2026, 10, 1, 12, 0) }),
    bot({ id: 'c', name: 'Cy', brainState: 'stopped', lastAt: null }),
  ];

  it('lists the status and the last message of each bot, newest first', () => {
    const events = activityEvents(bots);
    expect(events.map((e) => [e.botName, e.text])).toEqual([
      ['Bo', 'Last message: Hi there'],
      ['Ada', 'Needs your answer.'],
      ['Ada', 'Last message: Draft is ready'],
      ['Cy', 'Paused.'],
    ]);
    expect(events.find((e) => e.botId === 'c')?.at).toBeNull();
  });

  it('follows the search', () => {
    expect(activityEvents(bots, 'bo').map((e) => e.botId)).toEqual(['b']);
    expect(activityEvents(bots, 'nobody')).toEqual([]);
  });
});

describe('botMenuItems', () => {
  const ALL = ['pause', 'resume', 'markRead'] as const;

  it('offers Pause for a running bot and Mark read, greyed when nothing is unread', () => {
    expect(botMenuItems(bot({ brainState: 'ready', unread: 0 }), ALL)).toEqual([
      { action: 'pause', label: 'Pause', disabled: false },
      { action: 'markRead', label: 'Mark read', disabled: true },
    ]);
  });

  it('offers Resume for a stopped bot, and Mark read live when something is unread', () => {
    expect(botMenuItems(bot({ brainState: 'stopped', unread: 3 }), ALL)).toEqual([
      { action: 'resume', label: 'Resume', disabled: false },
      { action: 'markRead', label: 'Mark read', disabled: false },
    ]);
  });

  it('shows only the actions the page gave, and no Pause or Resume for a bot that is off', () => {
    expect(botMenuItems(bot({ brainState: 'ready', unread: 1 }), ['markRead']).map((i) => i.action)).toEqual(['markRead']);
    expect(botMenuItems(bot({ brainState: 'off', unread: 1 }), ALL).map((i) => i.action)).toEqual(['markRead']);
  });

  it('has no Rename or Delete: the app has no command for them yet', () => {
    const labels = botMenuItems(bot({ unread: 1 }), ALL).map((i) => i.label.toLowerCase());
    expect(labels.join(' ')).not.toMatch(/rename|delete/);
  });
});

describe('isSearchShortcut', () => {
  const key = (over: Partial<KeyboardEvent>) => ({ key: 'k', ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...over });

  it('is Ctrl+K or Cmd+K, alone', () => {
    expect(isSearchShortcut(key({ ctrlKey: true }))).toBe(true);
    expect(isSearchShortcut(key({ metaKey: true }))).toBe(true);
    expect(isSearchShortcut(key({ ctrlKey: true, key: 'K' }))).toBe(true);
  });

  it('is not a plain K, a chord with Shift or Alt, both at once, or another letter', () => {
    expect(isSearchShortcut(key({}))).toBe(false);
    expect(isSearchShortcut(key({ ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isSearchShortcut(key({ ctrlKey: true, altKey: true }))).toBe(false);
    expect(isSearchShortcut(key({ ctrlKey: true, metaKey: true }))).toBe(false);
    expect(isSearchShortcut(key({ ctrlKey: true, key: 'b' }))).toBe(false);
  });
});
