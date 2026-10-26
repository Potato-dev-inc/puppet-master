// @vitest-environment jsdom
// The Chat tab of a pane: shown for an agent pane (its session file), not for a plain shell, and it opens
// the read-only chat over that session file.
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
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, items: SAMPLE })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
}));

import { openSessionChat } from '../../lib/sessionChat';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const SAMPLE: ChatItem[] = [
  { kind: 'user', id: '10:0', at: null, text: 'List the files' },
  { kind: 'assistant', id: '20:0', at: null, text: 'Two entries.', model: null, finalAnswer: true },
];

function cockpit(agentType: string): CockpitApi {
  const p = { status: 'idle', info: { id: 'p1', agent_type: agentType, status: 'idle', pid: 4120, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 158, rows: 56 } };
  return {
    registry: { paneList: [p], panes: new Map([['p1', p]]), initialReady: true },
    names: new Map([['p1', 'Agent']]),
    activities: {},
    previews: {},
    asked: { questions: [] },
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
  vi.mocked(openSessionChat).mockClear();
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
const tabs = () => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].map((t) => t.textContent);
const tab = (name: string) => [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === name)!;

describe('pane Chat tab', () => {
  it('is shown for an agent pane, right after Terminal', async () => {
    await act(async () => root.render(<PaneView c={cockpit('claude')} paneId="p1" />));
    await flush();
    const names = tabs();
    expect(names.indexOf('Chat')).toBe(names.indexOf('Terminal') + 1);
  });

  it('is not shown for a plain shell pane', async () => {
    await act(async () => root.render(<PaneView c={cockpit('powershell')} paneId="p1" />));
    await flush();
    expect(tabs()).not.toContain('Chat');
    expect(tabs()).toContain('Terminal');
  });

  it('renders the chat panel when clicked, and reads the pane session file', async () => {
    await act(async () => root.render(<PaneView c={cockpit('claude')} paneId="p1" />));
    await flush();
    expect(host.querySelector('[data-testid="session-chat"]')).toBeNull();

    await act(async () => tab('Chat').click());
    await flush();
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
    expect(openSessionChat).toHaveBeenCalledWith('p1');
    const chat = host.querySelector('.ck-pane-body [data-testid="session-chat"]');
    expect(chat).not.toBeNull();
    expect(chat!.textContent).toContain('List the files');
    expect(chat!.textContent).toContain('Two entries.');
  });

  it('falls back to Terminal when the pane becomes a plain shell while Chat is open', async () => {
    const c = cockpit('claude');
    await act(async () => root.render(<PaneView c={c} paneId="p1" />));
    await flush();
    await act(async () => tab('Chat').click());
    await flush();
    expect(host.querySelector('[data-testid="session-chat"]')).not.toBeNull();

    const shell = cockpit('powershell');
    await act(async () => root.render(<PaneView c={shell} paneId="p1" />));
    await flush();
    expect(tabs()).not.toContain('Chat');
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    expect(host.querySelector('[data-testid="session-chat"]')).toBeNull();
  });
});
