// @vitest-environment jsdom
// The session rail beside the pane: shown on a wide window in every tab, hidden on a narrow one, toggled by the header button.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import { PaneView } from './PaneView';

const invoke = vi.fn(async (cmd: string) => {
  if (cmd === 'pane_git_branch') return 'rail-branch';
  if (cmd === 'session_chat_open') return { available: false, reason: 'No chat in this test', items: [] };
  if (cmd === 'checkpoints_list') return { state: 'ok', message: '', items: [] };
  return null;
});
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
  info: { id, agent_type: 'claude', status: 'idle', pid: 1, created_at: 1, last_output_at: null, cwd: '/work/shop', cols: 80, rows: 24, title, parent_pane_id: parent },
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

const setWidth = (w: number) => Object.defineProperty(window, 'innerWidth', { value: w, configurable: true });
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  window.localStorage.clear();
  setWidth(1400);
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
const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
const rail = () => host.querySelector('[data-testid="session-rail"]');
const toggle = () => host.querySelector<HTMLButtonElement>('button[aria-pressed]')!;

describe('session rail in the pane view', () => {
  it('shows the rail with the branch, the helpers and the parent', async () => {
    const c = cockpit([mk('p', 'Refactor auth'), mk('h', 'Check tests', 'p')], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(rail()).not.toBeNull();
    expect(rail()?.textContent).toContain('rail-branch');
    expect(rail()?.querySelector('[data-testid="rail-helpers"]')?.textContent).toContain('Check tests');
    await act(async () => rail()!.querySelector<HTMLButtonElement>('[data-testid="rail-helpers"] button')!.click());
    expect(c.select).toHaveBeenCalledWith('h');
  });

  it('says "Helper of" on a helper pane', async () => {
    const c = cockpit([mk('p', 'Refactor auth'), mk('h', 'Check tests', 'p')], 'h');
    await act(async () => root.render(<PaneView c={c} paneId="h" />));
    await flush();
    expect(rail()?.querySelector('.ck-rail-parent')?.textContent).toBe('Helper of Refactor auth');
  });

  it('stays in other tabs', async () => {
    const c = cockpit([mk('p', 'Refactor auth')], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    const receipt = Array.from(host.querySelectorAll<HTMLElement>('[role="tab"], button')).find((b) => b.textContent === 'Receipt')!;
    await act(async () => receipt.click());
    await flush();
    expect(rail()).not.toBeNull();
  });

  it('hides with the header button and remembers it', async () => {
    const c = cockpit([mk('p', 'Refactor auth')], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    expect(toggle().getAttribute('aria-label')).toBe('Hide session details');
    await act(async () => toggle().click());
    expect(rail()).toBeNull();
    expect(toggle().getAttribute('aria-label')).toBe('Show session details');
    expect(window.localStorage.getItem('pm.sessionRail.open')).toBe('0');
  });

  it('is not there on a narrow window, and has no button', async () => {
    setWidth(1000);
    const c = cockpit([mk('p', 'Refactor auth')], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    expect(rail()).toBeNull();
    expect(host.querySelector('button[aria-pressed]')).toBeNull();
  });
});
