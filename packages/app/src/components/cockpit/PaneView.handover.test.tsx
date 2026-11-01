// @vitest-environment jsdom
// The chat workspace in the pane view: "Open in terminal" and "Back to chat" (with the question before anything is
// stopped), the paused chat while its conversation is open in a terminal, the settings chips and the slash menu.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { AcpChatApi, AcpChatState, HandOver, OpenedInTerminal, BackInChat } from '../../lib/acpChat';
import { fakeAcpApi } from '../../lib/acpChatFake';
import type { ChatItem } from '../../lib/sessionChat';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, source: 'acp', items: SAMPLE })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
  onSessionChatStatus: vi.fn(async () => () => undefined),
  onSessionChatSource: vi.fn(async () => () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const SAMPLE: ChatItem[] = [
  { kind: 'user', id: 'u1', at: null, text: 'Write notes.txt' },
  { kind: 'assistant', id: 'a1.1', at: null, text: 'Asking first.', model: null, finalAnswer: false },
];

function cockpit(lane: 'acp' | undefined) {
  const info = { id: 'p1', agent_type: 'codex', status: 'idle', pid: lane ? 0 : 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 100, rows: 30, ...(lane ? { lane } : {}) };
  const p = { status: 'idle', info };
  const select = vi.fn();
  const notify = vi.fn();
  const c = {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    activities: {},
    previews: {},
    asked: { questions: [], answerQuestion: vi.fn(), dismissQuestion: vi.fn() },
    prompts: {},
    statusSince: new Map(),
    now: 12_000,
    selectedId: 'p1',
    mode: 'orchestration',
    select,
    notify,
    answer: vi.fn(),
    closePane: vi.fn(),
  } as unknown as CockpitApi;
  return { c, select, notify };
}

const state = (over: Partial<AcpChatState> = {}): AcpChatState => ({
  lane: 'acp',
  agent: 'codex',
  label: 'Codex',
  phase: 'idle',
  status: 'idle',
  turnInFlight: false,
  stopping: false,
  canSteer: false,
  turn: 1,
  title: null,
  usage: null,
  permissions: [],
  closed: false,
  conversation: 'p1',
  inTerminal: null,
  agentAnswering: false,
  configOptions: [],
  commands: [],
  ...over,
});

const opened = (): HandOver<OpenedInTerminal> => ({ ok: true, result: { conversation: 'p1', pane_id: 't1', agent: 'codex', session_id: 's1' } });
const back = (): HandOver<BackInChat> => ({ ok: true, result: { conversation: 'p1', session_id: 's1', how: 'resume' } });

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockImplementation(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null));
  host = document.createElement('div');
  host.className = 'cockpit';
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
});

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const byText = (text: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === text);
const testId = (id: string) => host.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`);

async function mount(c: CockpitApi, api: AcpChatApi) {
  await act(async () => root.render(<PaneView c={c} paneId="p1" acp={api} />));
  await flush();
}

describe('Open in terminal', () => {
  it('is a quiet button on a chat session; it hands the conversation over and shows the terminal pane', async () => {
    const { c, select } = cockpit('acp');
    const openInTerminal = vi.fn(async () => opened());
    await mount(c, fakeAcpApi({ state: async () => state(), openInTerminal }));
    await act(async () => testId('open-in-terminal')!.click());
    await flush();
    expect(openInTerminal).toHaveBeenCalledWith('p1', false);
    expect(select).toHaveBeenCalledWith('t1');
  });

  it('is not drawn for a chat that is not a saved conversation, nor on a terminal pane', async () => {
    const a = cockpit('acp');
    await mount(a.c, fakeAcpApi({ state: async () => state({ conversation: null }) }));
    expect(testId('open-in-terminal')).toBeNull();
    const b = cockpit(undefined);
    await mount(b.c, fakeAcpApi());
    expect(testId('open-in-terminal')).toBeNull();
  });

  it("asks first when the agent works, with the agent's own sentence, and the yes stops it", async () => {
    const { c, select } = cockpit('acp');
    const openInTerminal = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, ask: 'The agent is working in the chat', reason: 'turn_running' } satisfies HandOver<OpenedInTerminal>)
      .mockResolvedValueOnce(opened());
    await mount(c, fakeAcpApi({ state: async () => state(), openInTerminal }));
    await act(async () => testId('open-in-terminal')!.click());
    await flush();
    expect(select).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="handover-ask"]')?.textContent).toBe('The agent is working in the chat');
    expect(byText('Cancel')).toBeTruthy();
    await act(async () => byText('Stop it and open in terminal')!.click());
    await flush();
    expect(openInTerminal).toHaveBeenLastCalledWith('p1', true);
    expect(select).toHaveBeenCalledWith('t1');
    expect(host.querySelector('[data-testid="handover-ask"]')).toBeNull();
  });

  it('Cancel leaves everything as it was', async () => {
    const { c, select } = cockpit('acp');
    const openInTerminal = vi.fn(async (): Promise<HandOver<OpenedInTerminal>> => ({ ok: false, ask: 'The agent is working in the chat', reason: 'turn_running' }));
    await mount(c, fakeAcpApi({ state: async () => state(), openInTerminal }));
    await act(async () => testId('open-in-terminal')!.click());
    await flush();
    await act(async () => byText('Cancel')!.click());
    expect(openInTerminal).toHaveBeenCalledTimes(1);
    expect(select).not.toHaveBeenCalled();
    expect(host.querySelector('[data-testid="handover-ask"]')).toBeNull();
  });

  it('a refused hand-over says so in a sentence', async () => {
    const { c, notify } = cockpit('acp');
    const openInTerminal = vi.fn(async (): Promise<HandOver<OpenedInTerminal>> => {
      throw 'ACP_NO_SESSION: There is nothing to hand over yet.';
    });
    await mount(c, fakeAcpApi({ state: async () => state(), openInTerminal }));
    await act(async () => testId('open-in-terminal')!.click());
    await flush();
    expect(notify).toHaveBeenCalledWith('There is nothing to hand over yet.');
  });
});

describe('a conversation that is open in a terminal', () => {
  const handed = () => state({ inTerminal: { paneId: 't1' } });

  it('pauses the chat: a note, the way back, a link to the terminal, and no message box', async () => {
    const { c, select } = cockpit('acp');
    const backToChat = vi.fn(async () => back());
    await mount(c, fakeAcpApi({ state: async () => handed(), backToChat }));
    expect(host.querySelector('[data-testid="acp-handed"]')?.textContent).toContain('This conversation is open in a terminal');
    expect(testId('open-in-terminal')).toBeNull();
    const box = host.querySelector<HTMLTextAreaElement>('textarea');
    expect(box?.disabled).toBe(true);
    expect(host.textContent).toContain('Go back to chat to write here.');
    await act(async () => byText('Go to the terminal')!.click());
    expect(select).toHaveBeenCalledWith('t1');
    await act(async () => byText('Back to chat')!.click());
    await flush();
    expect(backToChat).toHaveBeenCalledWith('p1', false);
    expect(select).toHaveBeenLastCalledWith('p1');
  });

  it('asks before it stops the terminal', async () => {
    const { c } = cockpit('acp');
    const backToChat = vi
      .fn()
      .mockResolvedValueOnce({ ok: false, ask: 'The agent is working in the terminal', reason: 'busy' } satisfies HandOver<BackInChat>)
      .mockResolvedValueOnce(back());
    await mount(c, fakeAcpApi({ state: async () => handed(), backToChat }));
    await act(async () => byText('Back to chat')!.click());
    await flush();
    expect(host.querySelector('[data-testid="handover-ask"]')?.textContent).toBe('The agent is working in the terminal');
    await act(async () => byText('Stop the terminal and go back to chat')!.click());
    await flush();
    expect(backToChat).toHaveBeenLastCalledWith('p1', true);
  });

  it('a terminal pane that holds a conversation has Back to chat in its header, and it follows hand-overs', async () => {
    const { c, select } = cockpit(undefined);
    let held: { conversation: string; agent: string } | null = { conversation: 'chat-1', agent: 'codex' };
    let fire: (() => void) | null = null;
    const backToChat = vi.fn(async () => back());
    await mount(
      c,
      fakeAcpApi({
        conversationOfTerminal: async () => held,
        watchConversations: async (cb) => {
          fire = cb;
          return () => undefined;
        },
        backToChat,
      }),
    );
    expect(testId('back-to-chat')).not.toBeNull();
    await act(async () => testId('back-to-chat')!.click());
    await flush();
    expect(backToChat).toHaveBeenCalledWith('chat-1', false);
    expect(select).toHaveBeenCalledWith('chat-1');
    // The hand-over is done: the terminal holds no conversation any more.
    held = null;
    await act(async () => fire?.());
    await flush();
    expect(testId('back-to-chat')).toBeNull();
  });

  it('a terminal pane without a conversation has no such button', async () => {
    const { c } = cockpit(undefined);
    await mount(c, fakeAcpApi());
    expect(testId('back-to-chat')).toBeNull();
  });
});

describe('settings chips and slash commands in a chat session', () => {
  const options = [
    { id: 'model', name: 'Model', category: 'model', type: 'select', currentValue: 'a', options: [{ value: 'a', name: 'Fast' }, { value: 'b', name: 'Deep' }] },
    { id: 'mode', name: 'Mode', category: 'mode', type: 'select', currentValue: 'x', options: [{ value: 'x', name: 'Ask' }, { value: 'y', name: 'Auto' }] },
  ];

  it('draws one select per setting above the box and changes it through the backend', async () => {
    const { c } = cockpit('acp');
    const setConfig = vi.fn(async () => state());
    await mount(c, fakeAcpApi({ state: async () => state({ configOptions: options }), setConfig }));
    const chips = [...host.querySelectorAll<HTMLSelectElement>('[data-testid="acp-chips"] select')];
    expect(chips.map((s) => s.getAttribute('aria-label'))).toEqual(['Model', 'Mode']);
    expect(chips.every((s) => !s.disabled)).toBe(true);
    await act(async () => {
      chips[0].value = 'b';
      chips[0].dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(setConfig).toHaveBeenCalledWith('p1', 'model', 'b');
    expect(chips[0].value).toBe('b');
  });

  it('switches the chips off while a turn runs, and draws none when the agent offers none', async () => {
    const a = cockpit('acp');
    await mount(a.c, fakeAcpApi({ state: async () => state({ configOptions: options, turnInFlight: true, phase: 'working', status: 'running' }) }));
    const chips = [...host.querySelectorAll<HTMLSelectElement>('[data-testid="acp-chips"] select')];
    expect(chips).toHaveLength(2);
    expect(chips.every((s) => s.disabled)).toBe(true);
    const b = cockpit('acp');
    await mount(b.c, fakeAcpApi({ state: async () => state() }));
    expect(host.querySelector('[data-testid="acp-chips"]')).toBeNull();
  });

  it("a refused change takes the pick back and says why in the agent's sentence", async () => {
    const { c } = cockpit('acp');
    const setConfig = vi.fn(async (): Promise<AcpChatState> => {
      throw 'BUSY: The agent is working. Try again when it is done.';
    });
    await mount(c, fakeAcpApi({ state: async () => state({ configOptions: options }), setConfig }));
    const chip = host.querySelector<HTMLSelectElement>('[data-testid="acp-chips"] select')!;
    await act(async () => {
      chip.value = 'b';
      chip.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await flush();
    expect(host.querySelector('[data-testid="acp-chips"] [role="alert"]')?.textContent).toBe('The agent is working. Try again when it is done.');
    expect(host.querySelector<HTMLSelectElement>('[data-testid="acp-chips"] select')!.value).toBe('a');
  });

  it("gives the message box the agent's slash commands", async () => {
    const { c } = cockpit('acp');
    await mount(c, fakeAcpApi({ state: async () => state({ commands: [{ name: 'compact', description: 'Shrink the conversation', hint: null }] }) }));
    const box = host.querySelector<HTMLTextAreaElement>('textarea')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(box, '/');
      box.dispatchEvent(new Event('input', { bubbles: true }));
    });
    expect(host.querySelector('[role="listbox"]')?.textContent).toContain('/compact');
  });

  it('a terminal pane gets neither chips nor a slash menu', async () => {
    const { c } = cockpit(undefined);
    await mount(c, fakeAcpApi());
    expect(host.querySelector('[data-testid="acp-chips"]')).toBeNull();
  });
});
