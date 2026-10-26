// @vitest-environment jsdom
// A pane that works in a git worktree (isolation: "worktree" from the UI, the MCP or the bridge)
// gets its own Changes tab, so the diff and Discard are one click away. The live hunt found them
// only inside the Receipt tab, where nobody looked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CockpitApi } from '../../hooks/useCockpit';
import type { WorktreeItem } from '../../lib/cockpit-api';
import { PaneView } from './PaneView';
import { changesTabLabel } from './usePaneWorktree';

let worktreeFor: Record<string, WorktreeItem | null> = {};
let runInfo: { handle: string; status: string; receipt: unknown } | null = null;
const invoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
  if (cmd === 'pane_worktree') return worktreeFor[String(args?.paneId)] ?? null;
  if (cmd === 'pane_run_info') return runInfo;
  if (cmd === 'worktree_diff') {
    return {
      branch: 'pm/run-1',
      base: 'main',
      changes: { files: [{ path: 'notes.txt', status: 'modified', additions: 2, deletions: 0, binary: false }], files_changed: 1, additions: 2, deletions: 0, truncated: false },
      patch: '',
    };
  }
  return null;
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => (invoke as (...x: unknown[]) => unknown)(...(a as [string])) }));
vi.mock('../TerminalViewport', () => ({ TerminalViewport: () => null }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as { ResizeObserver?: unknown }).ResizeObserver = NoopResizeObserver;
Element.prototype.scrollIntoView = () => {};

const ITEM: WorktreeItem = {
  handle: 'run-1',
  branch: 'pm/run-1',
  base: 'main',
  repo: 'C:/proj',
  path: 'C:/proj/.puppet-master/worktrees/run-1-ab12',
  exists: true,
  state: 'unreviewed',
  summary: { files_changed: 1, additions: 2, deletions: 0 },
};

const pane = (id: string, status: string) => ({
  status,
  info: { id, agent_type: 'a', status, pid: 1, created_at: 1, last_output_at: null, cwd: 'C:/proj', cols: 80, rows: 24 },
});

function cockpit(): CockpitApi {
  const list = [pane('iso', 'idle'), pane('plain', 'idle')];
  return {
    registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true },
    names: new Map([['iso', 'Isolated'], ['plain', 'Plain']]),
    activities: {},
    previews: {},
    asked: { questions: [] },
    prompts: {},
    statusSince: new Map(),
    now: 0,
    selectedId: 'iso',
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
  worktreeFor = {};
  runInfo = null;
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

const tabs = () => [...host.querySelectorAll('[role="tab"]')].map((t) => t.textContent);
const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

describe('the pane header says where the agent works', () => {
  const headerLine = () => host.querySelector('.ck-pane-head .ck-sub-inline')!;
  const withCwd = (cwd: string): CockpitApi => {
    const c = cockpit();
    const list = [{ ...pane('iso', 'idle'), info: { ...pane('iso', 'idle').info, cwd } }];
    return { ...c, registry: { paneList: list, panes: new Map(list.map((p) => [p.info.id, p])), initialReady: true } } as unknown as CockpitApi;
  };

  it('names the folder when the agent works in it, with the full path on hover', async () => {
    await act(async () => root.render(<PaneView c={withCwd('C:\\work\\shop')} paneId="iso" />));
    await flush();
    expect(headerLine().textContent).toContain('shop');
    expect(headerLine().textContent).not.toContain('separate copy');
    expect(headerLine().getAttribute('title')).toBe('C:\\work\\shop');
  });

  it('says "separate copy of <folder>" when it works in a copy, and never shows the copy’s own folder', async () => {
    const copy = 'C:\\work\\shop\\.puppet-master\\worktrees\\pane-1a2b3c4d';
    await act(async () => root.render(<PaneView c={withCwd(copy)} paneId="iso" />));
    await flush();
    expect(headerLine().textContent).toContain('separate copy of shop');
    expect(headerLine().textContent).not.toContain('pane-1a2b3c4d');
    expect(headerLine().getAttribute('title')).toBe('C:\\work\\shop');
  });
});

describe('Changes tab for isolated panes', () => {
  it('shows a Changes tab with the file count, and Discard, for a pane in a worktree', async () => {
    worktreeFor = { iso: ITEM };
    await act(async () => root.render(<PaneView c={cockpit()} paneId="iso" />));
    await flush();
    expect(tabs()).toEqual(['Terminal', 'Chat', 'Changes · 1', 'Transcript', 'Receipt', 'History']);
    const tab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent?.startsWith('Changes'))!;
    await act(async () => tab.click());
    await flush();
    expect(host.querySelector('.ck-changes')).not.toBeNull();
    expect(host.textContent).toContain('notes.txt');
    const actions = [...host.querySelectorAll('.ck-changes-actions button')].map((b) => b.textContent);
    expect(actions).toEqual(expect.arrayContaining(['Merge', 'Discard']));
  });

  it('keeps the usual tabs for a pane that is not in a worktree', async () => {
    await act(async () => root.render(<PaneView c={cockpit()} paneId="plain" />));
    await flush();
    expect(tabs()).toEqual(['Terminal', 'Chat', 'Transcript', 'Receipt', 'History']);
  });

  it('the Receipt tab shows the result, and the run ID only under a closed Details disclosure', async () => {
    runInfo = { handle: 'run-zz9', status: 'running', receipt: { outcome: 'completed_unchecked', backend: 'claude' } };
    worktreeFor = { iso: ITEM };
    await act(async () => root.render(<PaneView c={cockpit()} paneId="iso" />));
    await flush();
    const receiptTab = [...host.querySelectorAll<HTMLElement>('[role="tab"]')].find((t) => t.textContent === 'Receipt')!;
    await act(async () => receiptTab.click());
    await flush();

    const details = host.querySelector<HTMLDetailsElement>('details.ck-details');
    expect(details).not.toBeNull();
    expect(details!.querySelector('summary')!.textContent).toBe('Details');
    expect(details!.open).toBe(false);
    expect(details!.textContent).toContain('run-zz9');
    expect(host.textContent).toContain('Finished, not verified');

    const outside = [...host.querySelectorAll('dt')].filter((dt) => !dt.closest('details'));
    expect(outside.map((dt) => dt.textContent)).not.toContain('Run ID');
    expect(outside.map((dt) => dt.textContent)).not.toContain('Status');
    expect(host.querySelector('.ck-receipt')!.textContent).not.toContain('Status');
  });

  it('labels the tab without a number until files changed', () => {
    expect(changesTabLabel({ ...ITEM, summary: null })).toBe('Changes');
    expect(changesTabLabel({ ...ITEM, summary: { files_changed: 0, additions: 0, deletions: 0 } })).toBe('Changes');
    expect(changesTabLabel(ITEM)).toBe('Changes · 1');
  });
});
