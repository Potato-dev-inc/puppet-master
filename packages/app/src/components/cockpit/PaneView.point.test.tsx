// @vitest-environment jsdom
// The pane header's Preview chip (opens the app in the in-app preview, Point and Fix ready), its browser
// button, and the Point and Fix draft that opens the pane's Chat tab.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { PreviewMap } from '../../lib/pane-preview';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
const listen = vi.fn(async (_event: string, _handler: unknown) => () => undefined);
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => (listen as (...x: unknown[]) => unknown)(...a) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));
vi.mock('../../lib/sessionChat', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../lib/sessionChat')>()),
  openSessionChat: vi.fn(async () => ({ available: true, reason: null, items: [] })),
  closeSessionChat: vi.fn(async () => undefined),
  onSessionChatItems: vi.fn(async () => () => undefined),
  onSessionChatStatus: vi.fn(async () => () => undefined),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const PREVIEWS: PreviewMap = { p1: [{ url: 'http://localhost:5173/', port: 5173 }] };

function cockpit(previews: PreviewMap): CockpitApi {
  const list = [1, 2].map((n) => ({
    status: 'idle',
    info: { id: `p${n}`, agent_type: 'a', status: 'idle', pid: n, created_at: 1, last_output_at: null, cwd: 'C:\\work\\shop', cols: 158, rows: 56 },
  }));
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([
      ['p1', 'Agent'],
      ['p2', 'Other'],
    ]),
    activities: {},
    previews,
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
  invoke.mockClear();
  listen.mockClear();
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
const commands = () => (invoke.mock.calls as unknown[][]).map((c) => c[0]);

describe('pane header Preview chip', () => {
  it('opens the app in the in-app preview for this pane and port', async () => {
    act(() => root.render(<PaneView c={cockpit(PREVIEWS)} paneId="p1" />));
    act(() => host.querySelector<HTMLButtonElement>('.ck-preview-chip')!.click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'p1', port: 5173 });
    expect(commands()).not.toContain('open_external_url');
  });

  it('says it is the place to point at something to fix', () => {
    act(() => root.render(<PaneView c={cockpit(PREVIEWS)} paneId="p1" />));
    expect(host.querySelector('.ck-preview-chip')?.getAttribute('title')).toBe('Preview · localhost:5173. Point at something to fix it.');
  });

  it('keeps Open in your browser on its own button', async () => {
    act(() => root.render(<PaneView c={cockpit(PREVIEWS)} paneId="p1" />));
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Open the preview in your browser"]')!.click());
    await flush();
    expect(invoke).toHaveBeenCalledWith('open_external_url', { url: 'http://localhost:5173/' });
    expect(commands()).not.toContain('point_open_preview');
  });

  it('has neither chip nor browser button without a preview', () => {
    act(() => root.render(<PaneView c={cockpit({})} paneId="p1" />));
    expect(host.querySelector('.ck-preview-chip')).toBeNull();
    expect(host.querySelector('[aria-label="Open the preview in your browser"]')).toBeNull();
  });
});

describe('Point and Fix draft', () => {
  const draftHandler = () => listen.mock.calls.find((c) => c[0] === 'point://draft')![1] as (e: { payload: { paneId: string } }) => void;

  it('opens the Chat tab of this pane, and only this pane', () => {
    act(() => root.render(<PaneView c={cockpit(PREVIEWS)} paneId="p1" />));
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    act(() => draftHandler()({ payload: { paneId: 'p2' } }));
    expect(tab('Terminal').getAttribute('aria-selected')).toBe('true');
    act(() => draftHandler()({ payload: { paneId: 'p1' } }));
    expect(tab('Chat').getAttribute('aria-selected')).toBe('true');
  });
});
