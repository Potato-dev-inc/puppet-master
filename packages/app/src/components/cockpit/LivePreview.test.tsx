// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { mainPreview, previewLabel, type PreviewMap } from '../../lib/pane-preview';
import { Palette } from './Palette';
import { PaneView } from './PaneView';
import { Sidebar } from './Sidebar';

const invoke = vi.fn(async () => null);
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

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockClear();
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

const pane = (id: string, status: string) => ({
  status,
  info: { id, agent_type: 'a', status, pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\work\site', cols: 80, rows: 24 },
});

const PREVIEWS: PreviewMap = { p1: [{ url: 'http://localhost:5173/', port: 5173, networkUrl: 'http://192.168.1.4:5173/' }, { url: 'http://localhost:3000', port: 3000 }] };

function cockpit(previews: PreviewMap): CockpitApi {
  const list = [pane('p1', 'running'), pane('p2', 'idle')];
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([['p1', 'Agent A'], ['p2', 'Agent B']]),
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

describe('preview helpers', () => {
  it('offers the newest preview and labels it with its port', () => {
    expect(mainPreview(PREVIEWS, 'p1')?.port).toBe(5173);
    expect(mainPreview(PREVIEWS, 'p2')).toBeNull();
    expect(mainPreview(undefined, 'p1')).toBeNull();
    expect(previewLabel(PREVIEWS.p1[0])).toBe(':5173');
  });
});

describe('sidebar', () => {
  it('shows a globe only on rows whose pane has a preview', () => {
    act(() => root.render(<Sidebar c={cockpit(PREVIEWS)} onOpenPalette={() => {}} />));
    const rows = [...host.querySelectorAll('.ck-row:not(.ck-row--flat)')];
    expect(rows.filter((r) => r.querySelector('.ck-row-preview')).length).toBe(1);
    expect(rows.find((r) => r.textContent?.includes('Agent A'))!.querySelector('.ck-row-preview')!.getAttribute('title')).toBe('Has a preview · localhost:5173');
  });
});

describe('pane header chip', () => {
  it('shows Preview with the port and opens the URL through open_external_url', () => {
    act(() => root.render(<PaneView c={cockpit(PREVIEWS)} paneId="p1" />));
    const chip = host.querySelector<HTMLButtonElement>('.ck-preview-chip')!;
    expect(chip.textContent).toContain('Preview');
    expect(chip.textContent).not.toContain(':5173');
    expect(chip.getAttribute('title')).toContain('localhost:5173');
    act(() => chip.click());
    expect(invoke).toHaveBeenCalledWith('open_external_url', { url: 'http://localhost:5173/' });
  });

  it('has no chip without a preview', () => {
    act(() => root.render(<PaneView c={cockpit({})} paneId="p1" />));
    expect(host.querySelector('.ck-preview-chip')).toBeNull();
  });
});

describe('command palette', () => {
  it('lists "Open preview: <agent> :port" for each preview and opens it', () => {
    act(() => root.render(<Palette c={cockpit(PREVIEWS)} onClose={() => {}} />));
    const titles = [...host.querySelectorAll('[role="option"], .ck-pal-item, button')].map((e) => e.textContent ?? '');
    expect(titles.some((t) => t.includes('Open preview: Agent A :5173'))).toBe(true);
    expect(titles.some((t) => t.includes('Open preview: Agent A :3000'))).toBe(true);
    const item = [...host.querySelectorAll<HTMLElement>('button, [role="option"]')].find((e) => e.textContent?.includes('Open preview: Agent A :5173'))!;
    act(() => item.click());
    expect(invoke).toHaveBeenCalledWith('open_external_url', { url: 'http://localhost:5173/' });
  });
});
