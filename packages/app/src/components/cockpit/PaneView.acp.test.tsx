// @vitest-environment jsdom
// A chat session over ACP in the pane view: the "Chat (ACP)" badge, the Chat tab as the pane (there is no terminal
// to show), and a message box that stays on after the agent stopped (the next message starts it again).
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
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

function cockpit(lane: 'acp' | undefined, status = 'idle', questions: unknown[] = []): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', status, pid: lane ? 0 : 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 100, rows: 30, ...(lane ? { lane } : {}) };
  const p = { status, info };
  return {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Codex']]),
    activities: {},
    previews: {},
    asked: { questions, answerQuestion: vi.fn(), dismissQuestion: vi.fn() },
    prompts: {},
    statusSince: new Map(),
    now: 12_000,
    selectedId: 'p1',
    mode: 'orchestration',
    select: vi.fn(),
    notify: vi.fn(),
    answer: vi.fn(),
    closePane: vi.fn(),
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

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === name)!;

describe('pane view of a chat session over ACP', () => {
  it('says "Chat (ACP)" on the pane and opens on its Chat tab', async () => {
    await act(async () => root.render(<PaneView c={cockpit('acp')} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="pane-lane"]')?.textContent).toBe('Chat (ACP)');
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
    expect(host.textContent).toContain('Write notes.txt');
    expect(host.querySelector('[data-testid="term"]')).toBeNull();
  });

  it('has no terminal to show: the Terminal tab says so instead of drawing an empty one', async () => {
    await act(async () => root.render(<PaneView c={cockpit('acp')} paneId="p1" />));
    await flush();
    await act(async () => tab('Terminal').click());
    expect(host.textContent).toContain('This is a chat session');
    expect(host.querySelector('[data-testid="term"]')).toBeNull();
  });

  it('a terminal pane has no badge and keeps its terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit(undefined)} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="pane-lane"]')).toBeNull();
    expect(host.querySelector('[data-testid="term"]')).not.toBeNull();
  });

  it("shows the agent's question under the conversation, and no screen fallback card", async () => {
    const question = {
      id: 'q1',
      question: 'Codex asks permission: Write notes.txt',
      choices: [{ id: 'allow_once', label: 'Allow Once' }, { id: 'reject_once', label: 'Reject' }],
      allow_free_text: false,
      urgency: 'high',
      created_at_ms: 1,
      expires_at_ms: 99,
      state: 'pending',
      kind: 'acp_permission',
      pane_id: 'p1',
    };
    await act(async () => root.render(<PaneView c={cockpit('acp', 'waiting_input', [question])} paneId="p1" />));
    await flush();
    expect(host.textContent).toContain('Codex asks permission: Write notes.txt');
    expect([...host.querySelectorAll('button')].some((b) => b.textContent === 'Allow Once')).toBe(true);
    expect(host.textContent).not.toContain('Looks stuck');
  });

  it('keeps the message box on when the agent stopped: the next message starts it again', async () => {
    await act(async () => root.render(<PaneView c={cockpit('acp', 'error')} paneId="p1" />));
    await flush();
    const box = host.querySelector<HTMLTextAreaElement>('textarea');
    expect(box).not.toBeNull();
    expect(box!.disabled).toBe(false);
  });
});
