// @vitest-environment jsdom
// The bot list: the Bots | Activity switch, search, the sections (Needs you, then Recent), the rows (face, name, App tag,
// time, last message, status line, unread dot, mood word), the row menu, the strip with its needs-you dot.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { BotList, type BotListActions, type BotListProps } from './BotList';
import { bot } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const NOW = new Date(2026, 10, 1, 15, 0).getTime();
const props = (over: Partial<BotListProps> = {}): BotListProps => ({
  bots: [],
  avatars: new Map<string, AvatarId>(),
  openId: null,
  onOpen: () => {},
  onNew: () => {},
  candidates: [],
  onUseCompanion: () => {},
  now: NOW,
  ...over,
});
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(label));
const rowOf = (name: string) => [...host.querySelectorAll<HTMLButtonElement>('.ck-potato-conv')].find((b) => b.querySelector('.ck-row-title')?.textContent === name);
const names = () => [...host.querySelectorAll('.ck-potato-conv .ck-row-title')].map((n) => n.textContent);
const sectionNames = () => [...host.querySelectorAll('.ck-potato-section')].map((s) => [...s.querySelectorAll('.ck-row-title')].map((n) => n.textContent));
const menu = () => host.querySelector<HTMLElement>('[role="menu"]');
const menuItem = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((b) => b.textContent === label);
const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m, d, h, min).getTime();

/** Types into the search box the way React hears it. */
function typeSearch(value: string) {
  const input = host.querySelector<HTMLInputElement>('input.ck-potato-search-input')!;
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value);
  act(() => {
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

/** Right-click on a row, the way the browser sends it. */
function rightClick(el: Element) {
  act(() => {
    el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 50 }));
  });
}

describe('BotList', () => {
  it('says there are no bots yet and offers New bot', () => {
    act(() => root.render(<BotList {...props()} />));
    expect(host.textContent).toContain('No bots yet. Make one to start chatting.');
    expect(button('New bot')).toBeDefined();
  });

  it('shows the name, the App tag, the last line, the time, the unread dot and the mood word', () => {
    const fish = bot({ lastLine: 'Fish heard: hello', lastAt: new Date(2026, 10, 1, 9, 5).getTime(), unread: 2, mood: { mood: 'genki', intensity: 3, energy: 60, hunger: 5 } });
    act(() => root.render(<BotList {...props({ bots: [fish], openId: 'fish' })} />));
    const row = host.querySelector('.ck-potato-conv')!;
    expect(row.querySelector('.ck-row-title')?.textContent).toBe('Fish');
    expect(row.querySelector('.ck-potato-tag')?.textContent).toBe('App');
    expect(row.querySelector('.ck-potato-last')?.textContent).toBe('Fish heard: hello');
    expect(row.querySelector('.ck-potato-time')?.textContent).toBe('9:05 AM');
    expect(row.querySelector('.ck-potato-mood-word')?.textContent).toBe('Genki');
    expect(row.querySelector('[aria-label="2 unread"]')).not.toBeNull();
    expect(row.getAttribute('aria-current')).toBe('true');
    expect(row.querySelector('svg[role="img"]')).not.toBeNull();
  });

  it('shows a tool line as a short summary, never the raw JSON', () => {
    const line = 'Fish used ask_person_for_screen: ok, saw {"reason":"login","url":"https://x.test"}';
    act(() => root.render(<BotList {...props({ bots: [bot({ lastLine: line })] })} />));
    expect(host.querySelector('.ck-potato-last')?.textContent).toBe('Used the computer');
    expect(host.textContent).not.toContain('{');
  });

  it('shows no dot when everything is read, and says when nothing was said yet', () => {
    act(() => root.render(<BotList {...props({ bots: [bot()] })} />));
    expect(host.querySelector('.ck-potato-dot')).toBeNull();
    expect(host.querySelector('.ck-potato-last')?.textContent).toBe('No messages yet');
  });

  it('shows the day in the time: Yesterday, then the weekday, then the date', () => {
    const bots = [
      bot({ id: 'y', name: 'Yes', lastAt: at(2026, 9, 31, 22, 0) }),
      bot({ id: 'w', name: 'Week', lastAt: at(2026, 9, 26, 10, 0) }),
      bot({ id: 'd', name: 'Date', lastAt: at(2026, 9, 3, 8, 0) }),
    ];
    act(() => root.render(<BotList {...props({ bots })} />));
    expect([...host.querySelectorAll('.ck-potato-time')].map((t) => t.textContent)).toEqual(['Yesterday', 'Monday', 'Oct 3']);
  });

  it('opens a bot when its row is clicked', () => {
    const onOpen = vi.fn();
    act(() => root.render(<BotList {...props({ bots: [bot(), bot({ id: 'b', name: 'Bee' })], onOpen })} />));
    act(() => button('Bee')!.click());
    expect(onOpen).toHaveBeenCalledWith('b');
  });

  it('is a strip of faces when it is small: no names, the same buttons by their labels, the unread dot kept', () => {
    const onToggleMini = vi.fn();
    act(() => root.render(<BotList {...props({ bots: [bot({ unread: 1 })], mini: true, onToggleMini, candidates: [{ id: 'mochi', name: 'Mochi' } as CompanionView] })} />));
    expect(host.querySelector('.ck-potato-list')?.classList.contains('is-mini')).toBe(true);
    expect(host.querySelector('.ck-row-title')).toBeNull();
    expect(host.querySelector('.ck-potato-conv')?.getAttribute('aria-label')).toBe('Fish');
    expect(host.querySelector('[aria-label="1 unread"]')).not.toBeNull();
    expect(host.querySelector('button[aria-label="New bot"]')).not.toBeNull();
    expect(host.querySelector('button[aria-label="Make Mochi a bot"]')).not.toBeNull();
    act(() => (host.querySelector('button[aria-label="Show the bot list"]') as HTMLButtonElement).click());
    expect(onToggleMini).toHaveBeenCalled();
  });

  it('has a button that makes the list small, only when the page gave it the way to', () => {
    const onToggleMini = vi.fn();
    act(() => root.render(<BotList {...props({ onToggleMini })} />));
    act(() => (host.querySelector('button[aria-label="Make the bot list smaller"]') as HTMLButtonElement).click());
    expect(onToggleMini).toHaveBeenCalled();
    act(() => root.render(<BotList {...props()} />));
    expect(host.querySelector('button[aria-label="Make the bot list smaller"]')).toBeNull();
  });

  it('opens the companion sheet from New bot and turns a companion into a bot with one click', () => {
    const onNew = vi.fn();
    const onUseCompanion = vi.fn();
    const mochi = { id: 'mochi', name: 'Mochi' } as CompanionView;
    act(() => root.render(<BotList {...props({ onNew, onUseCompanion, candidates: [mochi] })} />));
    act(() => button('New bot')!.click());
    act(() => button('Make Mochi a bot')!.click());
    expect(onNew).toHaveBeenCalled();
    expect(onUseCompanion).toHaveBeenCalledWith('mochi');
  });
});

describe('BotList search', () => {
  const bots = [bot({ id: 'fish', name: 'Fish', lastLine: 'Fish heard: hello' }), bot({ id: 'bee', name: 'Bee', lastLine: 'Bees buzz at dawn' })];

  it('filters by bot name and by the last message', () => {
    act(() => root.render(<BotList {...props({ bots })} />));
    typeSearch('buzz');
    expect(names()).toEqual(['Bee']);
    typeSearch('FISH');
    expect(names()).toEqual(['Fish']);
  });

  it('says so when nothing matches, and shows every bot again when the search is cleared', () => {
    act(() => root.render(<BotList {...props({ bots })} />));
    typeSearch('zzz');
    expect(names()).toEqual([]);
    expect(host.textContent).toContain('No bots match "zzz".');
    act(() => (host.querySelector('button[aria-label="Clear search"]') as HTMLButtonElement).click());
    expect(names()).toEqual(['Bee', 'Fish']);
  });

  it('Escape clears the search', () => {
    act(() => root.render(<BotList {...props({ bots })} />));
    typeSearch('bee');
    const input = host.querySelector<HTMLInputElement>('input.ck-potato-search-input')!;
    act(() => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(names()).toEqual(['Bee', 'Fish']);
  });

  it('Ctrl+K focuses the search box, and Cmd+K does too', () => {
    act(() => root.render(<BotList {...props({ bots })} />));
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }));
    });
    expect(document.activeElement).toBe(host.querySelector('input.ck-potato-search-input'));
    act(() => (document.activeElement as HTMLElement).blur());
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', metaKey: true, bubbles: true, cancelable: true }));
    });
    expect(document.activeElement).toBe(host.querySelector('input.ck-potato-search-input'));
  });

  it('Ctrl+K in the strip opens the list again first', () => {
    const onToggleMini = vi.fn();
    act(() => root.render(<BotList {...props({ bots, mini: true, onToggleMini })} />));
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }));
    });
    expect(onToggleMini).toHaveBeenCalledTimes(1);
  });

  it('is not in the strip', () => {
    act(() => root.render(<BotList {...props({ bots, mini: true })} />));
    expect(host.querySelector('input.ck-potato-search-input')).toBeNull();
  });
});

describe('BotList sections', () => {
  it('pins "Needs you" on top and then "Recent", both by recent', () => {
    const bots = [
      bot({ id: 'a', name: 'Ann', lastAt: at(2026, 10, 1, 9, 0) }),
      bot({ id: 'b', name: 'Bo', openCards: 1, lastAt: at(2026, 9, 30, 9, 0) }),
      bot({ id: 'c', name: 'Cy', lastAt: at(2026, 10, 1, 14, 0) }),
      bot({ id: 'd', name: 'Di', brainState: 'missing_program', lastAt: at(2026, 10, 1, 12, 0) }),
    ];
    act(() => root.render(<BotList {...props({ bots })} />));
    expect([...host.querySelectorAll('.ck-potato-section-title')].map((t) => t.textContent)).toEqual(['Needs you', 'Recent']);
    expect(sectionNames()).toEqual([
      ['Di', 'Bo'],
      ['Cy', 'Ann'],
    ]);
  });

  it('has no section titles when nothing needs you', () => {
    act(() => root.render(<BotList {...props({ bots: [bot({ id: 'a', name: 'Ann' }), bot({ id: 'b', name: 'Bo' })] })} />));
    expect(host.querySelector('.ck-potato-section-title')).toBeNull();
    expect(names()).toEqual(['Ann', 'Bo']);
  });

  it('shows a second line: Thinking, Waiting for you, Paused; and an error pill', () => {
    const bots = [
      bot({ id: 'w', name: 'Work', typing: true }),
      bot({ id: 'n', name: 'Need', openCards: 1 }),
      bot({ id: 'e', name: 'Err', lastError: 'PROVIDER_AUTH' }),
      bot({ id: 'p', name: 'Pause', brainState: 'stopped' }),
    ];
    act(() => root.render(<BotList {...props({ bots })} />));
    expect(rowOf('Work')?.querySelector('.ck-potato-status')?.textContent).toBe('Thinking…');
    expect(rowOf('Need')?.querySelector('.ck-potato-status')?.textContent).toBe('Waiting for you');
    expect(rowOf('Err')?.querySelector('.ck-potato-pill--quiet')?.textContent).toBe('Error');
    expect(rowOf('Pause')?.querySelector('.ck-potato-status')?.textContent).toBe('Paused');
  });
});

describe('BotList row menu', () => {
  const ACTIONS: BotListActions = { pause: vi.fn(), resume: vi.fn(), markRead: vi.fn() };
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('opens on right-click with Pause and Mark read, and Mark read is greyed when nothing is unread', () => {
    act(() => root.render(<BotList {...props({ bots: [bot({ unread: 0 })], actions: ACTIONS })} />));
    rightClick(rowOf('Fish')!);
    expect(menu()).not.toBeNull();
    expect(menuItem('Pause')).toBeDefined();
    expect((menuItem('Mark read') as HTMLButtonElement).disabled).toBe(true);
    expect(menuItem('Resume')).toBeUndefined();
  });

  it('pauses the bot from the menu and closes it', () => {
    act(() => root.render(<BotList {...props({ bots: [bot()], actions: ACTIONS })} />));
    rightClick(rowOf('Fish')!);
    act(() => menuItem('Pause')!.click());
    expect(ACTIONS.pause).toHaveBeenCalledWith('fish');
    expect(menu()).toBeNull();
  });

  it('resumes a stopped bot from the "..." button, and marks it read', () => {
    act(() => root.render(<BotList {...props({ bots: [bot({ brainState: 'stopped', unread: 4 })], actions: ACTIONS })} />));
    act(() => (host.querySelector('button.ck-potato-more') as HTMLButtonElement).click());
    expect(menuItem('Pause')).toBeUndefined();
    act(() => menuItem('Resume')!.click());
    expect(ACTIONS.resume).toHaveBeenCalledWith('fish');
    act(() => (host.querySelector('button.ck-potato-more') as HTMLButtonElement).click());
    act(() => menuItem('Mark read')!.click());
    expect(ACTIONS.markRead).toHaveBeenCalledWith('fish');
  });

  it('has no Rename or Delete', () => {
    act(() => root.render(<BotList {...props({ bots: [bot({ unread: 1 })], actions: ACTIONS })} />));
    act(() => (host.querySelector('button.ck-potato-more') as HTMLButtonElement).click());
    expect(host.querySelector('[role="menu"]')?.textContent).not.toMatch(/Rename|Delete/);
  });

  it('closes on Escape', () => {
    act(() => root.render(<BotList {...props({ bots: [bot()], actions: ACTIONS })} />));
    rightClick(rowOf('Fish')!);
    expect(menu()).not.toBeNull();
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(menu()).toBeNull();
  });

  it('is not there at all when the page gave the list no actions', () => {
    act(() => root.render(<BotList {...props({ bots: [bot()] })} />));
    expect(host.querySelector('button.ck-potato-more')).toBeNull();
    rightClick(rowOf('Fish')!);
    expect(menu()).toBeNull();
  });
});

describe('BotList Activity view', () => {
  const bots = [bot({ id: 'a', name: 'Ada', openCards: 1, lastLine: 'Draft is ready', lastAt: at(2026, 10, 1, 10, 0) }), bot({ id: 'b', name: 'Bo', lastLine: 'Hi', lastAt: at(2026, 10, 1, 12, 0) })];

  it('has a Bots | Activity switch, and Activity lists the events across the bots, newest first', () => {
    act(() => root.render(<BotList {...props({ bots })} />));
    const tabs = [...host.querySelectorAll<HTMLButtonElement>('.ck-potato-views .ck-seg-btn')];
    expect(tabs.map((t) => t.textContent)).toEqual(['Bots', 'Activity']);
    expect(tabs[0].getAttribute('aria-pressed')).toBe('true');
    act(() => tabs[1].click());
    expect(host.querySelector('.ck-potato-section-title')).toBeNull();
    const rows = [...host.querySelectorAll('.ck-potato-act')].map((r) => [r.querySelector('.ck-potato-act-name')?.textContent, r.querySelector('.ck-potato-act-text')?.textContent]);
    expect(rows).toEqual([
      ['Bo', 'Last message: Hi'],
      ['Ada', 'Needs your answer.'],
      ['Ada', 'Last message: Draft is ready'],
    ]);
  });

  it('says nothing has happened when no bot has an event yet, and opens a bot from its event', () => {
    const onOpen = vi.fn();
    const activityTab = () => host.querySelectorAll<HTMLButtonElement>('.ck-potato-views .ck-seg-btn')[1];
    act(() => root.render(<BotList {...props({ bots: [bot({ id: 'z', name: 'Zed' })], onOpen })} />));
    act(() => activityTab().click());
    expect(host.textContent).toContain('Nothing has happened yet.');
    act(() => root.render(<BotList {...props({ bots, onOpen })} />));
    act(() => host.querySelector<HTMLButtonElement>('.ck-potato-act')!.click());
    expect(onOpen).toHaveBeenCalledWith('b');
  });
});

describe('BotList strip with the needs-you dot', () => {
  it('puts a dot on a bot that needs you and a ringed "!" on one with an error, and the bots that need you first', () => {
    const bots = [bot({ id: 'a', name: 'Ada' }), bot({ id: 'b', name: 'Bo', openCards: 1 }), bot({ id: 'c', name: 'Cy', lastError: 'NETWORK' })];
    act(() => root.render(<BotList {...props({ bots, mini: true, onToggleMini: () => {} })} />));
    const faces = [...host.querySelectorAll<HTMLElement>('.ck-potato-conv')];
    expect(faces.map((f) => f.getAttribute('aria-label'))).toEqual(['Bo, Needs you', 'Cy, Error', 'Ada']);
    expect(host.querySelector('.ck-potato-dot--needs[aria-label="Needs you"]')).not.toBeNull();
    expect(host.querySelector('.ck-potato-alert-dot')?.textContent).toBe('!');
    expect(host.querySelector('[aria-label="Error"]')).not.toBeNull();
  });
});
