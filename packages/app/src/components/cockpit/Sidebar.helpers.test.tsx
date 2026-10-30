// @vitest-environment jsdom
// Helper panes in the sidebar: nested under the agent that started them, folded by a chip.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { ShipApi } from '../../lib/ship-api';
import { Sidebar } from './Sidebar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const pane = (id: string, status: string, title: string, parent?: string) => ({
  info: { id, agent_type: 'codex', pid: 1, status, created_at: 1, last_output_at: null, cwd: 'C:\work\shop', cols: 80, rows: 24, title, parent_pane_id: parent },
  status,
});

function api(list: ReturnType<typeof pane>[]) {
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map(list.map((p) => [p.info.id, 'Codex'])),
    asked: { questions: [] },
    launchEntries: [],
    readyAgents: new Set<string>(),
    mode: 'orchestration',
    theme: 'dark',
    selectedId: null,
    select: vi.fn(),
    prompts: {},
    statusSince: new Map(),
    now: 60_000,
    spawn: null,
    notify: vi.fn(),
    setMode: vi.fn(async () => {}),
    toggleTheme: vi.fn(async () => {}),
    openSettings: vi.fn(async () => {}),
    bridgeLine: '',
  } as unknown as CockpitApi;
}

const ship = { recent: vi.fn().mockResolvedValue([]), gitState: vi.fn().mockResolvedValue('repo') } as unknown as ShipApi;

async function render(c: CockpitApi) {
  await act(async () => root.render(<Sidebar c={c} onOpenPalette={() => {}} ship={ship} />));
  await act(async () => {});
}

const titles = (el: ParentNode = host) => [...el.querySelectorAll(el === host ? '.ck-group .ck-row-title' : '.ck-row-title')].map((t) => t.textContent);

describe('sidebar helpers', () => {
  it('nests helpers under their parent with a count chip', async () => {
    await render(api([pane('p', 'running', 'Refactor auth'), pane('h1', 'running', 'Check tests', 'p'), pane('h2', 'idle', 'Read docs', 'p')]));
    const group = host.querySelector('.ck-helpers')!;
    expect(titles(group)).toEqual(['Check tests', 'Read docs']);
    expect(host.querySelector('.ck-helper-chip')?.textContent).toBe('2 helpers');
    // Parent first, and the helpers are not also top-level rows of Idle.
    expect(titles()).toEqual(['Refactor auth', 'Check tests', 'Read docs']);
    expect(host.textContent).not.toContain('Idle');
  });

  it('folds and unfolds the helpers from the chip', async () => {
    await render(api([pane('p', 'running', 'Refactor auth'), pane('h1', 'running', 'Check tests', 'p')]));
    const chip = () => host.querySelector<HTMLButtonElement>('.ck-helper-chip')!;
    expect(chip().getAttribute('aria-expanded')).toBe('true');
    await act(async () => chip().click());
    expect(chip().getAttribute('aria-expanded')).toBe('false');
    expect(titles()).toEqual(['Refactor auth']);
    await act(async () => chip().click());
    expect(titles()).toEqual(['Refactor auth', 'Check tests']);
  });

  it('keeps a helper that needs you in Needs you, saying whose helper it is, even when folded', async () => {
    const c = api([pane('p', 'running', 'Refactor auth'), pane('h1', 'waiting_input', 'Check tests', 'p')]);
    await render(c);
    await act(async () => host.querySelector<HTMLButtonElement>('.ck-helper-chip')!.click());
    const needs = [...host.querySelectorAll('.ck-group')].find((g) => g.textContent?.startsWith('Needs you'))!;
    expect(titles(needs)).toEqual(['Check tests']);
    expect(needs.textContent).toContain('Helper of Refactor auth');
  });

  it('shows a helper whose parent is gone at top level, without a chip', async () => {
    await render(api([pane('h1', 'running', 'Check tests', 'closed')]));
    expect(host.querySelector('.ck-helper-chip')).toBeNull();
    expect(host.querySelector('.ck-helpers')).toBeNull();
    expect(titles()).toEqual(['Check tests']);
  });

  it('opens a helper by its row', async () => {
    const c = api([pane('p', 'running', 'Refactor auth'), pane('h1', 'running', 'Check tests', 'p')]);
    await render(c);
    const row = [...host.querySelectorAll<HTMLButtonElement>('.ck-helpers .ck-row')][0];
    await act(async () => row.click());
    expect(c.select).toHaveBeenCalledWith('h1');
  });
});

describe('sidebar helpers: the fold is remembered per parent', () => {
  const two = () => api([pane('p', 'running', 'Refactor auth'), pane('h1', 'running', 'Check tests', 'p'), pane('q', 'running', 'Write docs'), pane('h2', 'running', 'Read specs', 'q')]);

  it('keeps a folded parent folded after the sidebar is shown again, and leaves the other parent open', async () => {
    await render(two());
    const chips = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-helper-chip')];
    await act(async () => chips()[0].click());
    expect(titles()).toEqual(['Refactor auth', 'Write docs', 'Read specs']);
    await act(async () => root.unmount());
    root = createRoot(host);
    await render(two());
    expect(titles()).toEqual(['Refactor auth', 'Write docs', 'Read specs']);
    expect(chips().map((c) => c.getAttribute('aria-expanded'))).toEqual(['false', 'true']);
    await act(async () => chips()[0].click());
    expect(titles()).toEqual(['Refactor auth', 'Check tests', 'Write docs', 'Read specs']);
  });

  it('starts open when storage holds nothing usable', async () => {
    localStorage.setItem('pm.sidebar.helpersFolded', '{not json');
    await render(two());
    expect(titles()).toEqual(['Refactor auth', 'Check tests', 'Write docs', 'Read specs']);
  });
});
