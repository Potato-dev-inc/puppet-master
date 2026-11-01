// @vitest-environment jsdom
// The Stop button of a Chat (terminal) pane: while the agent works, the Chat tab's box shows Stop in place of Send.
// It calls the same Interrupt the pane menu uses, and Esc in the box does the same. Idle shows Send again.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, source: 'session', items: [] })),
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

function cockpit(lane: string | undefined, status: string, interrupt = vi.fn(async () => undefined)): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', status, pid: 4120, created_at: 1, last_output_at: null, cwd: 'C:\work\shop', cols: 100, rows: 30, ...(lane ? { lane } : {}) };
  const p = { status, info };
  return {
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
    select: vi.fn(),
    notify: vi.fn(),
    answer: vi.fn(),
    closePane: vi.fn(),
    interrupt,
  } as unknown as CockpitApi;
}

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

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const stopButton = () => host.querySelector<HTMLButtonElement>('[data-testid="chat-composer-stop"]');
const sendButton = () => host.querySelector<HTMLButtonElement>('button[type="submit"].ck-chat-send');
const field = () => host.querySelector<HTMLTextAreaElement>('[data-testid="chat-composer"] textarea')!;

describe('Stop in the Chat tab of a Chat (terminal) pane', () => {
  it('a busy Chat (terminal) pane shows Stop in place of Send', async () => {
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat', 'running')} paneId="p1" />));
    await flush();
    expect(stopButton()?.textContent).toContain('Stop');
    expect(sendButton()).toBeNull();
  });

  it('clicking Stop calls the same interrupt the pane menu uses, with the pane id', async () => {
    const interrupt = vi.fn(async () => undefined);
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat', 'running', interrupt)} paneId="p1" />));
    await flush();
    await act(async () => stopButton()!.click());
    expect(interrupt).toHaveBeenCalledTimes(1);
    expect(interrupt).toHaveBeenCalledWith('p1');
  });

  it('Esc in the message box while busy interrupts the agent', async () => {
    const interrupt = vi.fn(async () => undefined);
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat', 'running', interrupt)} paneId="p1" />));
    await flush();
    await act(async () => {
      field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(interrupt).toHaveBeenCalledWith('p1');
  });

  it('an idle Chat (terminal) pane shows Send, and Esc does nothing', async () => {
    const interrupt = vi.fn(async () => undefined);
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat', 'idle', interrupt)} paneId="p1" />));
    await flush();
    expect(stopButton()).toBeNull();
    expect(sendButton()?.textContent).toContain('Send');
    await act(async () => {
      field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    });
    expect(interrupt).not.toHaveBeenCalled();
  });

  it('after the agent stops, the box goes back to Send', async () => {
    const c = cockpit('terminal_chat', 'running');
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    expect(stopButton()).not.toBeNull();
    const p = { status: 'idle', info: { ...c.registry.panes.get('p1')!.info, status: 'idle' } };
    c.registry.panes.set('p1', p as never);
    c.registry.paneList[0] = p as never;
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    expect(stopButton()).toBeNull();
    expect(sendButton()).not.toBeNull();
  });

  it('a plain agent pane that is busy never shows Stop in its chat box', async () => {
    await act(async () => root.render(<PaneView c={cockpit(undefined, 'running')} paneId="p1" />));
    await flush();
    const chatTab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === 'Chat');
    if (chatTab) {
      await act(async () => chatTab.click());
      await flush();
    }
    expect(stopButton()).toBeNull();
  });
});
