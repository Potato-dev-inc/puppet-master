// @vitest-environment jsdom
// The host of the Bring my old chats sheets: no request, no sheet; a request opens its sheet; closing clears the request;
// Continue starts a pane with exactly the plan's agent, folder and arguments, selects it, asks for its Chat tab and
// closes the sheet; a failed start keeps the sheet and tells the person.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearBringChatsRequest,
  clearImportedChatRequest,
  idleProgress,
  requestBringChats,
  requestImportedChat,
  useBringChatsRequest,
  useImportedChatRequest,
  type BringChatsRequest,
  type ImportApi,
  type ImportedChatRequest,
  type ResumePlan,
} from '../../lib/chatImport';
import { clearChatTabRequest, useChatTabRequest, type ChatTabRequest } from '../../lib/chatTab';
import type { ChatItem } from '../../lib/sessionChatModel';
import { BringChatsHost, ImportHost } from './ImportHost';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = () => {};

const ITEMS: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '50:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
  { kind: 'turnEnd', id: '60:0', at: null, outcome: 'completed', durationMs: 1000 },
];
const PLAN: ResumePlan = { ok: true, agentType: 'codex', cwd: 'C:/Users/you/shop', extraArgs: ['resume', 'sess-0001-abcd'] };

function fakeApi(): ImportApi {
  return {
    state: vi.fn(async () => ({
      indexEnabled: true,
      sinceDays: 365,
      progress: idleProgress(),
      imported: { chats: 0, items: 0 },
    })),
    look: vi.fn(async () => []),
    start: vi.fn(async () => idleProgress()),
    cancel: vi.fn(async () => idleProgress()),
    dismiss: vi.fn(async () => {}),
    clear: vi.fn(async () => ({})),
    open: vi.fn(async () => ({
      meta: { title: 'Stripe webhook fix', agent: 'Codex', folder: 'shop', updatedMs: Date.UTC(2026, 7, 14), cutShort: false },
      items: ITEMS,
    })),
    resumePlan: vi.fn(async () => PLAN),
  } as unknown as ImportApi;
}

let host: HTMLDivElement;
let root: Root;
let imported: ImportedChatRequest | null;
let bring: BringChatsRequest | null;
let chatTab: ChatTabRequest | null;

function Watch() {
  imported = useImportedChatRequest();
  bring = useBringChatsRequest();
  chatTab = useChatTabRequest('new-pane');
  return null;
}

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
const button = (text: string) => [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);

type Host = Parameters<typeof ImportHost>[0]['c'];
function pane(): Host & { registry: { spawnPane: ReturnType<typeof vi.fn> }; select: ReturnType<typeof vi.fn>; notify: ReturnType<typeof vi.fn> } {
  return { registry: { spawnPane: vi.fn(async () => 'new-pane') }, select: vi.fn(), notify: vi.fn() } as never;
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => {
    if (imported) clearImportedChatRequest(imported);
    if (bring) clearBringChatsRequest(bring);
    if (chatTab) clearChatTabRequest(chatTab);
  });
  act(() => root.unmount());
  host.remove();
});

describe('ImportHost', () => {
  it('shows no sheet without a request', async () => {
    act(() => root.render(<><Watch /><ImportHost c={pane()} api={fakeApi()} /></>));
    await flush();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    expect(host.querySelector('.ck-scrim')).toBeNull();
  });

  it('an imported-chat request opens the old chat sheet', async () => {
    const api = fakeApi();
    act(() => root.render(<><Watch /><ImportHost c={pane()} api={api} /></>));
    act(() => requestImportedChat('codex', 'sess-0001-abcd', '50:0'));
    await flush();
    expect(host.querySelector('.history-viewer')).not.toBeNull();
    expect(api.open).toHaveBeenCalledWith('codex', 'sess-0001-abcd');
    expect(host.textContent).toContain('Stripe webhook fix');
    // An old chat has no live pane: the "Messages wait" note is not shown.
    expect(host.textContent).not.toContain('Messages wait');
  });

  it('closing the old chat sheet clears the request', async () => {
    act(() => root.render(<><Watch /><ImportHost c={pane()} api={fakeApi()} /></>));
    act(() => requestImportedChat('codex', 'sess-0001-abcd'));
    await flush();
    expect(imported).not.toBeNull();
    act(() => button('Close')!.click());
    expect(imported).toBeNull();
    expect(host.querySelector('.history-viewer')).toBeNull();
  });

  it('Continue starts a pane with exactly the plan, selects it, asks for its Chat tab and closes the sheet', async () => {
    const c = pane();
    act(() => root.render(<><Watch /><ImportHost c={c} api={fakeApi()} /></>));
    act(() => requestImportedChat('codex', 'sess-0001-abcd'));
    await flush();
    await act(async () => {
      button('Continue in a pane')!.click();
    });
    await flush();
    expect(c.registry.spawnPane).toHaveBeenCalledTimes(1);
    expect(c.registry.spawnPane).toHaveBeenCalledWith({ agent_type: 'codex', cwd: 'C:/Users/you/shop', extra_args: ['resume', 'sess-0001-abcd'] });
    expect(c.select).toHaveBeenCalledWith('new-pane');
    expect(chatTab?.paneId).toBe('new-pane');
    expect(imported).toBeNull();
    expect(host.querySelector('.history-viewer')).toBeNull();
  });

  it('a failed start keeps the sheet open and says so', async () => {
    const c = pane();
    c.registry.spawnPane.mockRejectedValueOnce(new Error('no such agent'));
    act(() => root.render(<><Watch /><ImportHost c={c} api={fakeApi()} /></>));
    act(() => requestImportedChat('codex', 'sess-0001-abcd'));
    await flush();
    await act(async () => {
      button('Continue in a pane')!.click();
    });
    await flush();
    expect(c.select).not.toHaveBeenCalled();
    expect(c.notify).toHaveBeenCalledWith('Could not start the agent: no such agent');
    expect(imported).not.toBeNull();
    expect(chatTab).toBeNull();
  });

  it('a Bring-chats request opens the panel and closing clears it', async () => {
    act(() => root.render(<><Watch /><ImportHost c={pane()} api={fakeApi()} /></>));
    expect(host.querySelector('.bc-panel')).toBeNull();
    act(() => requestBringChats());
    await flush();
    expect(host.querySelector('.bc-panel')).not.toBeNull();
    expect(host.querySelector('[role="dialog"]')).not.toBeNull();
    act(() => button('Close')!.click());
    expect(bring).toBeNull();
    expect(host.querySelector('.bc-panel')).toBeNull();
  });
});

describe('BringChatsHost (the Settings window)', () => {
  it('shows nothing without a request, and the panel in a styled wrapper with one', async () => {
    act(() => root.render(<><Watch /><BringChatsHost api={fakeApi()} standalone /></>));
    expect(host.querySelector('[data-testid="bring-chats-standalone"]')).toBeNull();
    act(() => requestBringChats());
    await flush();
    const wrap = host.querySelector('[data-testid="bring-chats-standalone"]');
    expect(wrap?.classList.contains('cockpit')).toBe(true);
    expect(wrap?.querySelector('.bc-panel')).not.toBeNull();
  });

  it('inside the cockpit it adds no wrapper', async () => {
    act(() => root.render(<><Watch /><BringChatsHost api={fakeApi()} /></>));
    act(() => requestBringChats());
    await flush();
    expect(host.querySelector('[data-testid="bring-chats-standalone"]')).toBeNull();
    expect(host.querySelector('.bc-panel')).not.toBeNull();
  });
});
