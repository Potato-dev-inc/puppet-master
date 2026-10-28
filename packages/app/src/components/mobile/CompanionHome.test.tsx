// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../../lib/bridge-types';
import { fakeCompanionTransport, type CompanionView, type FeedItem, type MemoryView } from '../../lib/companion';
import type { PhoneAbilities } from '../../lib/phone-abilities';
import { CompanionHome } from './CompanionHome';

// The chat itself has its own tests (PhoneChatView.test.tsx); here only what the home passes to it is checked.
vi.mock('./PhoneChatView', async () => {
  const React = await import('react');
  return {
    PhoneChatView: (props: { paneId: string; canType: boolean; canWatch: boolean }) =>
      React.createElement('div', {
        'data-testid': 'chat',
        'data-pane': props.paneId,
        'data-can-type': String(props.canType),
        'data-can-watch': String(props.canWatch),
      }),
  };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FEED_KEY = 'pm.mobile.companion.feedMark.mochi';
const ALL: PhoneAbilities = { canWatch: true, canType: true, canAnswer: true, canControl: true, typingNote: null };

const view = (over: Partial<CompanionView> = {}): CompanionView => ({
  id: 'mochi',
  name: 'Mochi',
  avatar: 'mochi',
  persona: 'calm',
  agentType: 'claude',
  agentLabel: 'Claude Code',
  folderName: 'proj',
  state: 'idle',
  stateLine: 'Here. Ask me anything.',
  paneId: 'companion-mochi',
  needs: null,
  pendingMemory: 0,
  nextCheckin: null,
  restarts: 0,
  autoStart: true,
  ...over,
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function mount(
  props: {
    companion?: Partial<CompanionView>;
    abilities?: Partial<PhoneAbilities>;
    chatAllowed?: boolean;
    feed?: FeedItem[];
    memory?: Partial<MemoryView>;
  } = {},
) {
  const companion = view(props.companion);
  const transport = fakeCompanionTransport({ companions: [companion], feed: props.feed, memory: props.memory });
  const onOpenCard = vi.fn();
  const onChanged = vi.fn();
  await act(async () => {
    root.render(
      <CompanionHome
        companion={companion}
        bridge={{} as BridgeClient}
        transport={transport}
        abilities={{ ...ALL, ...props.abilities }}
        chatAllowed={props.chatAllowed ?? true}
        onOpenCard={onOpenCard}
        onChanged={onChanged}
      />,
    );
  });
  return { transport, onOpenCard, onChanged };
}

const buttonByText = (text: string) => Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find((b) => b.textContent === text);

describe('CompanionHome', () => {
  it('shows the name, the state line and the face with the state word', async () => {
    await mount({ companion: { state: 'working', stateLine: 'Working…' } });
    expect(host.querySelector('h1')?.textContent).toBe('Mochi');
    expect(host.querySelector('.ph-comp-state')?.textContent).toBe('Working…');
    expect(host.querySelector('svg[role="img"]')?.getAttribute('aria-label')).toBe('Mochi, Working');
  });

  it('shows the agent it runs on now, and when a move ends', async () => {
    await mount({ companion: { agentLabel: 'Codex' } });
    expect(host.querySelector('.ph-comp-agent')?.textContent).toBe('On Codex');
    await act(async () => root.unmount());
    root = createRoot(host);
    await mount({ companion: { agentLabel: 'Codex', previousAgent: 'claude', untilMs: new Date(2026, 9, 28, 17, 0, 0).getTime() } });
    // "17:00" today, "Wed 17:00" on another day: the phone's feed clock.
    expect(host.querySelector('.ph-comp-agent')?.textContent).toMatch(/^On Codex until (\w{3} )?17:00$/);
  });

  it('Open card opens the card the companion waits on', async () => {
    const { onOpenCard } = await mount({ companion: { state: 'needs_you', needs: { itemId: 'card-9', title: 'Run tests?' } } });
    act(() => buttonByText('Open card')!.click());
    expect(onOpenCard).toHaveBeenCalledWith('card-9');
  });

  it('shows the chat with the companion pane, and passes the phone caps to it', async () => {
    await mount({ abilities: { canType: false, typingNote: 'This phone can watch, not type.' } });
    const chat = host.querySelector('[data-testid="chat"]');
    expect(chat?.getAttribute('data-pane')).toBe('companion-mochi');
    expect(chat?.getAttribute('data-can-type')).toBe('false');
    expect(chat?.getAttribute('data-can-watch')).toBe('true');
  });

  it('an agent that cannot take chat gets one plain line instead of the chat', async () => {
    await mount({ chatAllowed: false });
    expect(host.querySelector('[data-testid="chat"]')).toBeNull();
    expect(host.textContent).toContain("Chat can't send messages to this agent yet.");
  });

  it('sleeping shows Wake up {name} in place of the chat, and Wake up sends wake', async () => {
    const { transport, onChanged } = await mount({ companion: { state: 'sleeping', stateLine: 'Sleeping. Wake me when you need me.' } });
    expect(host.querySelector('[data-testid="chat"]')).toBeNull();
    const wake = buttonByText('Wake up Mochi')!;
    expect(wake).toBeDefined();
    await act(async () => wake.click());
    expect(transport.calls.map((c) => c.method)).toContain('wake');
    expect(onChanged).toHaveBeenCalledOnce();
  });

  it('hides Sleep and Wake up without the control cap', async () => {
    await mount({ abilities: { canControl: false }, companion: { state: 'sleeping' } });
    expect(buttonByText('Wake up Mochi')).toBeUndefined();
    expect(buttonByText('Sleep')).toBeUndefined();
  });

  it('Sleep sends sleep and passes the new view up', async () => {
    const { transport, onChanged } = await mount();
    await act(async () => buttonByText('Sleep')!.click());
    expect(transport.calls.map((c) => c.method)).toContain('sleep');
    expect(onChanged).toHaveBeenCalledWith(expect.objectContaining({ id: 'mochi', state: 'sleeping' }));
  });

  it('shows the next check-in in the footer with its clock', async () => {
    const eight = new Date();
    eight.setHours(8, 0, 0, 0);
    await mount({ companion: { nextCheckin: { id: 'c1', label: 'Morning brief', atMs: eight.getTime() } } });
    expect(host.querySelector('.ph-comp-foot')?.textContent).toContain('Next: Morning brief · 8:00');
  });

  it('feed shows only the items newer than the stored mark, and stores the newest one', async () => {
    localStorage.setItem(FEED_KEY, '1000');
    await mount({
      feed: [
        { id: 'a', atMs: 500, kind: 'reply', text: 'Old one' },
        { id: 'b', atMs: 1500, kind: 'checkin', text: 'Asked "Morning brief"' },
        { id: 'c', atMs: 2000, kind: 'reply', text: 'Newest reply' },
      ],
    });
    const texts = Array.from(host.querySelectorAll('.ph-comp-feed-row .ph-comp-text')).map((el) => el.textContent);
    expect(texts).toEqual(['Newest reply', 'Asked "Morning brief"']);
    expect(localStorage.getItem(FEED_KEY)).toBe('2000');
  });

  it('works when localStorage throws: the feed still shows and nothing is stored', async () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    await mount({ feed: [{ id: 'a', atMs: 700, kind: 'reply', text: 'Still here' }] });
    expect(host.querySelector('.ph-comp-feed-row')?.textContent).toContain('Still here');
    expect(host.querySelector('[data-testid="chat"]')).not.toBeNull();
  });

  it('notes wait for the answer cap: Keep and Don\'t keep are hidden without it', async () => {
    const pending = [{ id: 'p1', kind: 'note' as const, text: 'Likes tea', flagged: null, targetLine: null }];
    await mount({ abilities: { canAnswer: false }, memory: { pending } });
    expect(host.textContent).toContain('Likes tea');
    expect(buttonByText('Keep')).toBeUndefined();
  });

  it('Keep sends the decision and shows the notes the PC returns', async () => {
    const pending = [{ id: 'p1', kind: 'note' as const, text: 'Likes tea', flagged: null, targetLine: null }];
    const { transport } = await mount({ memory: { pending } });
    await act(async () => buttonByText('Keep')!.click());
    expect(transport.calls.find((c) => c.method === 'decidePending')?.args).toEqual(['mochi', 'p1', 'keep']);
    expect(host.textContent).not.toContain('Likes tea');
  });
});
