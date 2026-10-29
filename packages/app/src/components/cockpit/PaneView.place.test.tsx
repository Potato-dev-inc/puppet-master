// @vitest-environment jsdom
// A pane that runs in Linux (WSL) shows a `Linux · <distro>` chip next to the account label. A Windows pane shows none.
// The chip's tooltip names the distro and the folder, then one line per warning.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { PanePlace } from '../../lib/wsl';
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

const linux: PanePlace = {
  kind: 'linux',
  distro: 'Ubuntu-Test',
  cwd: '/mnt/c/work/shop',
  mount_root: '/mnt/',
  unc_root: '\\wsl.localhost\Ubuntu-Test',
};

const mk = (id: string, place?: PanePlace) => ({
  status: 'idle',
  info: { id, agent_type: 'claude', status: 'idle', pid: 1, created_at: 1, last_output_at: null, cwd: 'C:\work\shop', cols: 80, rows: 24, title: 'Refactor auth', place },
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
const chip = () => host.querySelector<HTMLElement>('[data-testid="pane-place"]');

describe('pane place chip', () => {
  it('shows Linux and the distro for a Linux pane', async () => {
    const c = cockpit([mk('p', linux)], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(chip()?.textContent).toBe('Linux · Ubuntu-Test');
    expect(chip()?.className).toBe('ck-sub-inline');
  });

  it('shows no chip for a Windows pane', async () => {
    const c = cockpit([mk('p')], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(chip()).toBeNull();
    expect(host.textContent).not.toContain('Linux ·');
  });

  it('tooltip names the distro and the folder', async () => {
    const c = cockpit([mk('p', linux)], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(chip()?.title).toBe('Runs in Ubuntu-Test (WSL) in /mnt/c/work/shop');
  });

  it('tooltip lists one line per warning', async () => {
    const c = cockpit([mk('p', { ...linux, warnings: ['Line endings may differ.', 'Git hooks do not run here.'] })], 'p');
    await act(async () => root.render(<PaneView c={c} paneId="p" />));
    await flush();
    expect(chip()?.title).toBe('Runs in Ubuntu-Test (WSL) in /mnt/c/work/shop\nLine endings may differ.\nGit hooks do not run here.');
  });
});
