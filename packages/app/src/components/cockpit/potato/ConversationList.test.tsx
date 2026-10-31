// @vitest-environment jsdom
// The 240 px list: one row per bot with face, name, App tag, last line, time, unread dot and mood word.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AvatarId, CompanionView } from '../../../lib/companion';
import { ConversationList, type ConversationListProps } from './ConversationList';
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
const props = (over: Partial<ConversationListProps> = {}): ConversationListProps => ({
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

describe('ConversationList', () => {
  it('says there are no bots yet and offers New bot', () => {
    act(() => root.render(<ConversationList {...props()} />));
    expect(host.textContent).toContain('No bots yet. Make one to start chatting.');
    expect(button('New bot')).toBeDefined();
  });

  it('shows the name, the App tag, the last line, the time, the unread dot and the mood word', () => {
    const fish = bot({ lastLine: 'Fish heard: hello', lastAt: new Date(2026, 10, 1, 9, 5).getTime(), unread: 2, mood: { mood: 'genki', intensity: 3, energy: 60, hunger: 5 } });
    act(() => root.render(<ConversationList {...props({ bots: [fish], openId: 'fish' })} />));
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
    act(() => root.render(<ConversationList {...props({ bots: [bot()] })} />));
    expect(host.querySelector('.ck-potato-dot')).toBeNull();
    expect(host.querySelector('.ck-potato-last')?.textContent).toBe('No messages yet');
  });

  it('opens a bot when its row is clicked', () => {
    const onOpen = vi.fn();
    act(() => root.render(<ConversationList {...props({ bots: [bot(), bot({ id: 'b', name: 'Bee' })], onOpen })} />));
    act(() => button('Bee')!.click());
    expect(onOpen).toHaveBeenCalledWith('b');
  });

  it('opens the companion sheet from New bot and turns a companion into a bot with one click', () => {
    const onNew = vi.fn();
    const onUseCompanion = vi.fn();
    const mochi = { id: 'mochi', name: 'Mochi' } as CompanionView;
    act(() => root.render(<ConversationList {...props({ onNew, onUseCompanion, candidates: [mochi] })} />));
    act(() => button('New bot')!.click());
    act(() => button('Make Mochi a bot')!.click());
    expect(onNew).toHaveBeenCalled();
    expect(onUseCompanion).toHaveBeenCalledWith('mochi');
  });
});
