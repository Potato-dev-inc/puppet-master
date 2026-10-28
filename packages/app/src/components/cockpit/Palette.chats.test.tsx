// @vitest-environment jsdom
// Ctrl+Shift+F: the palette searches what was said in every chat.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import * as chatSearch from '../../lib/chatSearch';
import type { ChatHit } from '../../lib/chatSearch';
import { clearChatJump, useChatJump, type ChatJump } from '../../lib/chatJump';
import { Palette } from './Palette';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.useFakeTimers();
  Element.prototype.scrollIntoView = vi.fn();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function hit(extra: Partial<ChatHit> = {}): ChatHit {
  return {
    paneId: 'p1',
    session: 's1',
    itemId: '50:0',
    kind: 'assistant',
    agent: 'codex',
    folder: 'shop',
    atMs: Date.now() - 5 * 60_000,
    snippet: 'We moved the deploy script to scripts/deploy.sh',
    marks: [[13, 19]],
    ...extra,
  };
}

function answer(hits: ChatHit[], enabled = true): chatSearch.ChatSearchAnswer {
  return { enabled, hits };
}

function cockpit(extra: Partial<CockpitApi> = {}): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', status: 'idle', cwd: 'C:\\work\\shop', title: null };
  return {
    registry: { paneList: [{ info, status: 'idle' }], panes: new Map([['p1', { info, status: 'idle' }]]) },
    launchEntries: [],
    readyAgents: new Set(),
    names: new Map([['p1', 'Codex']]),
    selectedId: null,
    mode: 'orchestration',
    theme: 'dark',
    previews: {},
    openSettings: vi.fn(async () => {}),
    notify: vi.fn(),
    select: vi.fn(),
    ...extra,
  } as unknown as CockpitApi;
}

const input = () => host.querySelector<HTMLInputElement>('input')!;
const options = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];

function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(input(), text);
    input().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(200);
  });
}

function press(key: string) {
  act(() => {
    input().dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
}

let jump: ChatJump | null = null;
function JumpProbe() {
  jump = useChatJump('p1');
  return null;
}

function open(c: CockpitApi, onClose = vi.fn()) {
  act(() =>
    root.render(
      <>
        <Palette c={c} onClose={onClose} initialMode="chats" />
        <JumpProbe />
      </>,
    ),
  );
  return onClose;
}

describe('Palette: Search all chats', () => {
  it('opens in chat search, asks for nothing until something is typed, and says what it searches', () => {
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([]));
    open(cockpit());
    expect(input().getAttribute('aria-label')).toBe('Search all chats');
    expect(host.textContent).toContain('Type to search every chat you have opened.');
    expect(search).not.toHaveBeenCalled();
  });

  it('lists hits with who spoke, the marked words, the agent, the folder and when', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit(), hit({ itemId: '10:0', kind: 'user', snippet: 'move it', marks: [[0, 4]], atMs: Date.now() - 3 * 86_400_000 })]));
    open(cockpit());
    type('deploy');
    await settle();
    const rows = options();
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelector('.ck-hit-who')?.textContent).toBe('Agent');
    expect(rows[0].querySelector('mark')?.textContent).toBe('deploy');
    expect(rows[0].querySelector('.ck-hit-meta')?.textContent).toBe('Codex CLI · shop · 5 min ago');
    expect(rows[1].querySelector('.ck-hit-who')?.textContent).toBe('You');
    expect(rows[1].querySelector('mark')?.textContent).toBe('move');
  });

  it('says No matches when nothing was found', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([]));
    open(cockpit());
    type('zebra');
    await settle();
    expect(host.textContent).toContain('No matches');
  });

  it('Enter opens the pane and asks its chat to scroll to the message', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit()]));
    const c = cockpit();
    const onClose = open(c);
    type('deploy');
    await settle();
    press('Enter');
    expect(onClose).toHaveBeenCalled();
    expect(c.select).toHaveBeenCalledWith('p1');
    expect(jump).toMatchObject({ paneId: 'p1', itemId: '50:0' });
    act(() => clearChatJump(jump!));
  });

  it('arrow keys move the choice', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit(), hit({ paneId: 'p1', itemId: '60:0' })]));
    const c = cockpit();
    open(c);
    type('deploy');
    await settle();
    press('ArrowDown');
    press('Enter');
    expect(jump?.itemId).toBe('60:0');
    act(() => clearChatJump(jump!));
  });

  it('a hit whose pane is gone opens a read-only card, not a pane', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit({ paneId: 'gone' })]));
    const c = cockpit();
    const onClose = open(c);
    type('deploy');
    await settle();
    press('Enter');
    expect(onClose).not.toHaveBeenCalled();
    expect(c.select).not.toHaveBeenCalled();
    expect(host.textContent).toContain('This chat is closed, so this is all that was kept.');
    expect(host.querySelector('.ck-hit-detail mark')?.textContent).toBe('deploy');
    // Esc goes back to the results first, then closes.
    press('Escape');
    expect(host.querySelector('.ck-hit-detail')).toBeNull();
    expect(options()).toHaveLength(1);
    press('Escape');
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('the commands list has a "Search all chats" row that switches over without closing', () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([]));
    const onClose = vi.fn();
    act(() => root.render(<Palette c={cockpit()} onClose={onClose} />));
    const row = options().find((r) => r.querySelector('.ck-palette-title')?.textContent === 'Search all chats')!;
    expect(row.querySelector('kbd')?.textContent).toBe('Ctrl+Shift+F');
    act(() => row.click());
    expect(onClose).not.toHaveBeenCalled();
    expect(input().getAttribute('aria-label')).toBe('Search all chats');
  });

  it('Backspace on an empty box goes back to the commands', () => {
    open(cockpit());
    press('Backspace');
    expect(input().getAttribute('aria-label')).toBe('Search commands');
  });

  it('says so when the search fails', async () => {
    vi.spyOn(chatSearch, 'searchChats').mockRejectedValue(new Error('x'));
    open(cockpit());
    type('deploy');
    await settle();
    expect(host.textContent).toContain('Search is not available right now.');
  });

  it('says search is off, and where to turn it on, when the person turned the index off', async () => {
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([], false));
    open(cockpit());
    type('deploy');
    await settle();
    expect(search).toHaveBeenCalled();
    expect(host.textContent).toContain('Search is off — turn it on in Settings');
    expect(host.textContent).not.toContain('No matches');
    expect(options()).toHaveLength(0);
  });
});

describe('Palette: filters in Search all chats', () => {
  const chipChoices = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-chat-chip-choice')];
  const click = (el: Element) => act(() => (el as HTMLElement).click());
  const chipNamed = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('.ck-chat-chip')].find((b) => b.textContent?.startsWith(label))!;
  const facetsOf = (agents: string[], folders: string[]) => vi.spyOn(chatSearch, 'chatSearchFacets').mockResolvedValue({ agents, folders });
  async function flush() {
    await act(async () => {
      await Promise.resolve();
    });
  }

  it('shows the four chips under the box while searching chats', () => {
    vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([]));
    open(cockpit());
    expect([...host.querySelectorAll('.ck-chat-chip')].map((b) => b.textContent)).toEqual(['Agent ▾', 'Folder ▾', 'When ▾', 'Who ▾']);
  });

  it('a typed agent: shortcut is taken out of the words and sent as the agent filter', async () => {
    facetsOf(['codex'], ['shop']);
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit()]));
    open(cockpit());
    await flush();
    type('agent:codex deploy');
    await settle();
    expect(search).toHaveBeenLastCalledWith('deploy', undefined, { agent: 'codex' });
    expect(options()).toHaveLength(1);
  });

  it('a chip alone searches the newest items that pass it, with no words', async () => {
    facetsOf([], ['shop']);
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit({ snippet: 'We moved the deploy script' })]));
    open(cockpit());
    await flush();
    click(chipNamed('Folder'));
    click(chipChoices()[1]);
    await settle();
    expect(search).toHaveBeenLastCalledWith('', undefined, { folder: 'shop' });
    expect(host.textContent).not.toContain('Type to search every chat you have opened.');
    expect(options()).toHaveLength(1);
  });

  it('from:me alone lists what the person said, and the When chip becomes a start time', async () => {
    vi.spyOn(chatSearch, 'chatSearchFacets').mockResolvedValue({ agents: [], folders: [] });
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([hit({ kind: 'user' })]));
    open(cockpit());
    await flush();
    type('from:me');
    await settle();
    expect(search).toHaveBeenLastCalledWith('', undefined, { role: 'you' });
    click(chipNamed('When'));
    click(chipChoices()[2]);
    await settle();
    const last = search.mock.lastCall!;
    expect(last[0]).toBe('');
    expect(last[2]).toMatchObject({ role: 'you' });
    expect(typeof last[2]?.sinceMs).toBe('number');
  });

  it('picking a chip for an agent takes its typed shortcut out of the box', async () => {
    facetsOf(['codex'], []);
    const search = vi.spyOn(chatSearch, 'searchChats').mockResolvedValue(answer([]));
    open(cockpit());
    await flush();
    type('agent:codex deploy');
    await settle();
    click(chipNamed('Agent'));
    click(chipChoices()[0]);
    await settle();
    expect(input().value).toBe('deploy');
    expect(search).toHaveBeenLastCalledWith('deploy', undefined, {});
  });
});
