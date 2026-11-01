// @vitest-environment jsdom
// A Chat (terminal) pane in the pane view: it opens on its Chat tab with a grey "Chat (terminal)" badge, and the
// Terminal tab stays one click away (it is a normal terminal pane). A plain terminal pane is unchanged.
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

function cockpit(lane: string | undefined): CockpitApi {
  const info = { id: 'p1', agent_type: 'codex', status: 'idle', pid: 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 100, rows: 30, ...(lane ? { lane } : {}) };
  const p = { status: 'idle', info };
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
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === name)!;

describe('pane view of a Chat (terminal) pane', () => {
  it('opens on the Chat tab and shows the grey "Chat (terminal)" badge', async () => {
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat')} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="pane-lane"]')?.textContent).toBe('Chat (terminal)');
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
    expect(host.textContent).not.toContain('ACP');
  });

  it('keeps the Terminal tab one click away, and it still shows the terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit('terminal_chat')} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="term"]')).toBeNull();
    await act(async () => tab('Terminal').click());
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-testid="term"]')).not.toBeNull();
    expect(host.textContent).not.toContain('This is a chat session');
  });

  it('a plain terminal pane has no badge and opens on its terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit(undefined)} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="pane-lane"]')).toBeNull();
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-testid="term"]')).not.toBeNull();
  });
});
