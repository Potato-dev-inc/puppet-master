// @vitest-environment jsdom
// The Chat tab of a pane whose agent started helpers: a compact "Helper: <task>" row per helper, a click opens it.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ChatItem } from '../../lib/sessionChat';
import { requestChatTab } from '../../lib/chatTab';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, items: SAMPLE })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
  onSessionChatStatus: vi.fn(async () => () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = () => {};

const T0 = Date.parse('2026-10-30T10:00:00Z');
const SAMPLE: ChatItem[] = [
  { kind: 'user', id: '1', at: '2026-10-30T10:00:00.000Z', text: 'Map the app' },
  { kind: 'assistant', id: '2', at: '2026-10-30T10:00:10.000Z', text: 'Starting helpers.', model: null, finalAnswer: false },
];

const mk = (id: string, status: string, title: string, parent?: string, createdAt = 1) => ({
  status,
  info: { id, agent_type: 'codex', status, pid: 1, created_at: createdAt, last_output_at: null, cwd: 'C:\\work\\shop', cols: 80, rows: 24, title, parent_pane_id: parent },
});

function cockpit(list: ReturnType<typeof mk>[], selected: string, questions: { pane_id: string }[] = []): CockpitApi {
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map(list.map((p) => [p.info.id, 'Codex'])),
    activities: {},
    previews: {},
    asked: { questions },
    prompts: {},
    statusSince: new Map(),
    now: 12_000,
    selectedId: selected,
    mode: 'orchestration',
    select: vi.fn(),
    notify: vi.fn(),
  } as unknown as CockpitApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
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
const rows = () => [...host.querySelectorAll<HTMLElement>('[data-testid="pane-helper-row"]')];

async function openChat(c: CockpitApi, paneId: string) {
  requestChatTab(paneId);
  await act(async () => root.render(<PaneView c={c} paneId={paneId} />));
  await flush();
}

describe('pane chat: helpers it started', () => {
  it('lists each helper with its task, agent and status, and opens one on a click', async () => {
    const c = cockpit(
      [
        mk('p', 'running', 'Lead', undefined, T0 - 60_000),
        mk('h1', 'running', 'Map app frontend', 'p', T0 + 20_000),
        mk('h2', 'idle', 'Map app backend', 'p', T0 + 30_000),
        mk('h3', 'running', 'Check the schema', 'p', T0 + 40_000),
        mk('x', 'running', 'Someone else', 'other', T0 + 50_000),
      ],
      'p',
      [{ pane_id: 'h3' }],
    );
    await openChat(c, 'p');
    expect(rows().map((r) => r.textContent)).toEqual([
      'Helper: Map app frontendCodexworking',
      'Helper: Map app backendCodexdone',
      'Helper: Check the schemaCodexneeds you',
    ]);
    await act(async () => rows()[1].querySelector<HTMLButtonElement>('button')!.click());
    expect(c.select).toHaveBeenCalledWith('h2');
  });

  it('shows no helper rows for a pane that started none', async () => {
    await openChat(cockpit([mk('p', 'running', 'Lead', undefined, T0), mk('solo', 'running', 'Solo', undefined, T0 + 1000)], 'p'), 'p');
    expect(host.querySelector('[data-testid="session-chat"]')).not.toBeNull();
    expect(rows()).toEqual([]);
  });
});
