// @vitest-environment jsdom
// The bot list: one row per bot with face, name, App tag, last line, time, unread dot and mood word.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { BotList, type BotListProps } from './BotList';
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
    expect(row.querySelector('.ck-potato-time')?.textContent).toMatch(/9:05/);
    expect(row.querySelector('.ck-potato-mood-word')?.textContent).toBe('Genki');
    expect(row.querySelector('[aria-label="2 unread"]')).not.toBeNull();
    expect(row.getAttribute('aria-current')).toBe('true');
    expect(row.querySelector('svg[role="img"]')).not.toBeNull();
  });

  it('shows no dot when everything is read, and says when nothing was said yet', () => {
    act(() => root.render(<BotList {...props({ bots: [bot()] })} />));
    expect(host.querySelector('.ck-potato-dot')).toBeNull();
    expect(host.querySelector('.ck-potato-last')?.textContent).toBe('No messages yet');
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
