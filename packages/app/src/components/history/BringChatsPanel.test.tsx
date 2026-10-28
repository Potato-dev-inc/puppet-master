// @vitest-environment jsdom
// "Bring my old chats" panel: fake api, fake progress events, no agent folder touched.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as chatImport from '../../lib/chatImport';
import type { ImportApi, ImportFound, ImportProgress, ImportState } from '../../lib/chatImport';
import * as chatIndex from '../../lib/chatIndex';
import { BringChatsPanel, BRING_REMOVE_BODY } from './BringChatsPanel';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

const CLAUDE: ImportFound = {
  id: 'claude',
  label: 'Claude Code',
  fromDefinition: false,
  chats: 412,
  bytes: 38 * 1024 * 1024,
  oldestMs: null,
  newestMs: null,
  already: 0,
  tooBig: 0,
  places: ['~/.claude/projects'],
  canContinue: true,
};
const CODEX: ImportFound = { ...CLAUDE, id: 'codex', label: 'Codex', chats: 126, bytes: 2 * 1024 * 1024, places: ['~/.codex/sessions'] };
const ACME: ImportFound = { ...CLAUDE, id: 'acme', label: 'Acme agent', fromDefinition: true, chats: 9, places: ['~/.acme/sessions'] };

const IDLE: ImportProgress = { phase: 'idle', agent: null, chatsDone: 0, chatsTotal: 0, items: 0, unchanged: 0, tooBig: 0, unreadable: 0, message: null };

function state(over: Partial<ImportState> = {}): ImportState {
  return {
    asked: true,
    sinceDays: 365,
    indexEnabled: true,
    agents: [
      { id: 'claude', label: 'Claude Code', fromDefinition: false, enabled: true },
      { id: 'codex', label: 'Codex', fromDefinition: false, enabled: true },
    ],
    progress: IDLE,
    imported: { chats: 0, items: 0 },
    ...over,
  };
}

function fakeApi(over: Partial<ImportApi> = {}, st: ImportState = state(), found: ImportFound[] = [CLAUDE, CODEX]): ImportApi {
  return {
    state: vi.fn(async () => st),
    look: vi.fn(async () => found),
    start: vi.fn(async () => ({ ...IDLE, phase: 'reading' as const, chatsTotal: 96 })),
    cancel: vi.fn(async () => ({ ...IDLE, phase: 'cancelled' as const })),
    dismiss: vi.fn(async () => undefined),
    clear: vi.fn(async () => ({ enabled: true, items: 0, bytes: 0, importedChats: 0, importedItems: 0 })),
    open: vi.fn(),
    resumePlan: vi.fn(),
    ...over,
  };
}

let progressCallback: ((p: ImportProgress) => void) | null = null;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  progressCallback = null;
  vi.spyOn(chatImport, 'onImportProgress').mockImplementation((cb) => {
    progressCallback = cb;
    return () => {};
  });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(api: ImportApi) {
  await act(async () => {
    root.render(<BringChatsPanel api={api} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
}

async function click(element: Element | null | undefined) {
  await act(async () => {
    (element as HTMLElement | null)?.click();
  });
  await act(async () => {
    await Promise.resolve();
  });
}

const text = () => host.textContent ?? '';
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);
const switches = () => [...host.querySelectorAll<HTMLButtonElement>('button[role="switch"]')];
const findMyChats = () => button('Find my chats');

describe('BringChatsPanel', () => {
  it('the intro touches no agent folder: only state is called', async () => {
    const api = fakeApi();
    await render(api);
    expect(text()).toContain('Read my old chats on this PC. Nothing leaves this computer.');
    expect(api.state).toHaveBeenCalledTimes(1);
    expect(api.look).not.toHaveBeenCalled();
    expect(api.start).not.toHaveBeenCalled();
  });

  it('Find my chats looks with the saved window and lists each agent with its folder', async () => {
    const api = fakeApi();
    await render(api);
    await click(findMyChats());
    expect(api.look).toHaveBeenCalledWith(365);
    expect(text()).toContain('Claude Code · 412 chats · 38 MB');
    expect(text()).toContain('Reads ~/.claude/projects');
    expect(switches()).toHaveLength(2);
    expect(switches().every((s) => s.getAttribute('aria-checked') === 'true')).toBe(true);
  });

  it('toggling an agent changes the button count and the start arguments', async () => {
    const api = fakeApi();
    await render(api);
    await click(findMyChats());
    expect(button('Add 538 chats to search')).toBeTruthy();
    await click(switches()[1]);
    expect(button('Add 412 chats to search')).toBeTruthy();
    await click(button('Add 412 chats to search'));
    expect(api.start).toHaveBeenCalledWith(['claude'], 365);
  });

  it('a definition agent starts off, with its folder line', async () => {
    const api = fakeApi({}, state(), [CLAUDE, ACME]);
    await render(api);
    await click(findMyChats());
    const acme = switches()[1];
    expect(acme.getAttribute('aria-checked')).toBe('false');
    expect(text()).toContain('From your agent definition');
    expect(text()).toContain('Reads ~/.acme/sessions');
    expect(button('Add 412 chats to search')).toBeTruthy();
  });

  it('progress events update the line and the bar', async () => {
    const api = fakeApi();
    await render(api);
    await click(findMyChats());
    await click(button('Add 538 chats to search'));
    expect(progressCallback).not.toBeNull();
    await act(async () => {
      progressCallback?.({ ...IDLE, phase: 'reading', agent: 'codex', chatsDone: 31, chatsTotal: 96, unchanged: 2 });
    });
    expect(text()).toContain('Reading Codex chats… 31 of 96');
    const bar = host.querySelector('[role="progressbar"]');
    expect(bar?.getAttribute('aria-valuenow')).toBe('31');
    expect(bar?.getAttribute('aria-valuemax')).toBe('96');
    expect(text()).toContain('Skipped 2');
    expect(text()).not.toContain('Too big');
  });

  it('Stop calls cancel', async () => {
    const api = fakeApi();
    await render(api);
    await click(findMyChats());
    await click(button('Add 538 chats to search'));
    await click(button('Stop'));
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it('when chat search is off, it says so and Turn on switches it on', async () => {
    const turnOn = vi.spyOn(chatIndex, 'chatIndexSetEnabled').mockResolvedValue({ enabled: true, items: 0, bytes: 0 });
    const api = fakeApi({}, state({ indexEnabled: false }));
    await render(api);
    expect(text()).toContain('Chat search is off. Turn it on to bring your old chats.');
    expect(findMyChats()).toBeUndefined();
    await click(button('Turn on'));
    expect(turnOn).toHaveBeenCalledWith(true);
    expect(api.look).not.toHaveBeenCalled();
  });

  it('a finished run shows what was added, and Look again returns to the list', async () => {
    const api = fakeApi({}, state({ progress: { ...IDLE, phase: 'done', chatsDone: 538, chatsTotal: 538 } }));
    await render(api);
    expect(text()).toContain('Added 538 chats. Press Ctrl+Shift+F to find anything in them.');
    await click(button('Look again'));
    expect(api.look).toHaveBeenCalledWith(365);
    expect(switches()).toHaveLength(2);
  });

  it('Remove imported chats asks first and clears only on Remove', async () => {
    const api = fakeApi({}, state({ progress: { ...IDLE, phase: 'done', chatsDone: 5, chatsTotal: 5 } }));
    await render(api);
    await click(button('Remove imported chats'));
    expect(text()).toContain(BRING_REMOVE_BODY);
    expect(api.clear).not.toHaveBeenCalled();
    await click(button('Cancel'));
    expect(api.clear).not.toHaveBeenCalled();
    await click(button('Remove imported chats'));
    await click(button('Remove'));
    expect(api.clear).toHaveBeenCalledTimes(1);
    expect(text()).toContain('Old chats removed from search');
  });

  it('changing the time window looks again with the new window', async () => {
    const api = fakeApi();
    await render(api);
    await click(findMyChats());
    const select = host.querySelector('select') as HTMLSelectElement;
    await act(async () => {
      select.value = '30';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(api.look).toHaveBeenLastCalledWith(30);
  });
});
