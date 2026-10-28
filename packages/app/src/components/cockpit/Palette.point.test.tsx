// @vitest-environment jsdom
// The command palette's preview actions: "Open preview" opens the app in the in-app preview, and
// "Point and fix in the preview" opens the same preview with the picker on.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { PreviewMap } from '../../lib/pane-preview';
import { Palette } from './Palette';

const invoke = vi.fn(async () => null as unknown);
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...a) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const PREVIEWS: PreviewMap = { p1: [{ url: 'http://localhost:5173/', port: 5173 }] };

const pane = (id: string) => ({
  status: 'running',
  info: { id, agent_type: 'a', status: 'running', pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\\work\\site', cols: 80, rows: 24 },
});

function cockpit(previews: PreviewMap): CockpitApi {
  const list = [pane('p1')];
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([['p1', 'Agent A']]),
    activities: {},
    previews,
    asked: { questions: [] },
    prompts: {},
    statusSince: new Map(),
    now: 0,
    selectedId: 'p1',
    spawn: null,
    mode: 'orchestration',
    theme: 'dark',
    launchEntries: [],
    readyAgents: new Set(),
    bridgeLine: '',
    select: vi.fn(),
    notify: vi.fn(),
  } as unknown as CockpitApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  invoke.mockImplementation(async () => null);
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
const commands = () => (invoke.mock.calls as unknown[][]).map((c) => c[0]);
const itemWith = (text: string) => [...host.querySelectorAll<HTMLElement>('button, [role="option"]')].find((e) => e.textContent?.includes(text)) ?? null;

describe('command palette preview actions', () => {
  it('lists "Point and fix in the preview" for each preview', () => {
    act(() => root.render(<Palette c={cockpit(PREVIEWS)} onClose={() => {}} />));
    expect(itemWith('Point and fix in the preview: Agent A :5173')).not.toBeNull();
  });

  it('Point and fix opens the preview and arms the picker', async () => {
    act(() => root.render(<Palette c={cockpit(PREVIEWS)} onClose={() => {}} />));
    act(() => itemWith('Point and fix in the preview: Agent A :5173')!.click());
    await flush();
    expect(commands()).toEqual(['point_open_preview', 'point_arm']);
    expect(invoke).toHaveBeenCalledWith('point_open_preview', { paneId: 'p1', port: 5173 });
    expect(commands()).not.toContain('open_external_url');
  });

  it('Open preview opens the app preview without arming the picker', async () => {
    act(() => root.render(<Palette c={cockpit(PREVIEWS)} onClose={() => {}} />));
    act(() => itemWith('Open preview: Agent A :5173')!.click());
    await flush();
    expect(commands()).toEqual(['point_open_preview']);
  });

  it('says why when the preview is gone', async () => {
    const c = cockpit(PREVIEWS);
    invoke.mockImplementation(async () => {
      throw 'That preview is not running any more.';
    });
    act(() => root.render(<Palette c={c} onClose={() => {}} />));
    act(() => itemWith('Point and fix in the preview: Agent A :5173')!.click());
    await flush();
    expect(vi.mocked(c.notify)).toHaveBeenCalledWith('That preview is not running any more.');
  });
});
