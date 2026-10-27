// @vitest-environment jsdom
// A helper pane's header names the pane that started it and opens it on a click.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => (cmd === 'checkpoints_list' ? { state: 'ok', message: '', items: [] } : null) as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => <div data-testid="term" /> }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = () => {};

const mk = (id: string, title: string, parent?: string) => ({
  status: 'idle',
  info: { id, agent_type: 'claude', status: 'idle', pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\work\shop', cols: 80, rows: 24, title, parent_pane_id: parent },
});

function cockpit(list: ReturnType<typeof mk>[], selected: string): CockpitApi {
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map(list.map((p) => [p.info.id, 'Claude Code'])),
    activities: {},
    previews: {},
    asked: { questions: [] },
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

describe('helper pane header', () => {
  it('names the parent and opens it', async () => {
    const c = cockpit([mk('p', 'Refactor auth'), mk('h', 'Check tests', 'p')], 'h');
    await act(async () => root.render(<PaneView c={c} paneId="h" />));
    await flush();
    const link = host.querySelector<HTMLButtonElement>('.ck-helper-of')!;
    expect(link.textContent).toBe('Helper of Refactor auth');
    await act(async () => link.click());
    expect(c.select).toHaveBeenCalledWith('p');
  });

  it('shows nothing for a pane with no live parent', async () => {
    const c = cockpit([mk('p', 'Refactor auth'), mk('h', 'Check tests', 'gone')], 'h');
    await act(async () => root.render(<PaneView c={c} paneId="h" />));
    await flush();
    expect(host.querySelector('.ck-helper-of')).toBeNull();
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(host.querySelector('.ck-helper-of')).toBeNull();
  });
});
