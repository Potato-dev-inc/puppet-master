// @vitest-environment jsdom
// Changes tab, keeping a branch copy: the Merge confirm warns when the copy is a branch whose original chat may
// have kept going. A normal copy, a kept branch or a lookup that fails shows no warning. Merge is never blocked.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import type { WorktreeDiff, WorktreeItem } from '../../lib/cockpit-api';
import { branchLineage, type BranchLineage, type BranchView } from '../../lib/branch-from';
import { Changes, type ChangesApi } from './Changes';

vi.mock('../../lib/branch-from', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/branch-from')>();
  return { ...actual, branchLineage: vi.fn() };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const WARNING =
  'This branch started after turn 5 · 14:02. The original chat may have kept working after that, and merging this branch does not bring that later work in. Check both chats before you merge.';

const ITEM: WorktreeItem = {
  handle: 'run-1',
  branch: 'pm/run-1',
  base: 'main',
  repo: 'C:/proj',
  path: 'C:/proj/.puppet-master/worktrees/run-1-ab12',
  exists: true,
  state: 'unreviewed',
  summary: null,
};

const DIFF: WorktreeDiff = {
  branch: 'pm/run-1',
  base: 'main',
  changes: {
    files: [{ path: 'a.txt', status: 'modified', additions: 1, deletions: 0, binary: false }],
    files_changed: 1,
    additions: 1,
    deletions: 0,
    truncated: false,
  },
  patch: '',
  patch_truncated: false,
};

const BRANCH: BranchView = {
  id: 'bf-0123456789ab',
  fromLabel: 'Fake A',
  toLabel: 'Fake B',
  fromPaneId: 'pane-a',
  toPaneId: 'p1',
  atMs: 1_000,
  pointLabel: 'after turn 5 · 14:02',
  state: 'running',
  stateText: 'Running in a separate copy',
  turns: 2,
  briefBytes: 100,
  branchName: 'pm/bf-0123456789ab',
  closed: null,
  failedFiles: 0,
};

function fakeApi(): ChangesApi {
  return {
    paneWorktree: vi.fn().mockResolvedValue(ITEM),
    worktreeDiff: vi.fn().mockResolvedValue(DIFF),
    worktreeMerge: vi.fn().mockResolvedValue({ merged: true, merge_commit: 'abc', files: [], branch: 'pm/run-1', into: 'main' }),
    worktreeDiscard: vi.fn().mockResolvedValue(undefined),
  } as ChangesApi;
}

const lineageMock = branchLineage as Mock;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  lineageMock.mockReset();
  lineageMock.mockResolvedValue({ from: null, children: [] } satisfies BranchLineage);
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function mount(api: ChangesApi) {
  await act(async () => {
    root.render(<Changes paneId="p1" status="idle" api={api} />);
  });
  await act(async () => {});
}

const click = async (el: Element | null) => {
  await act(async () => {
    (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
};
const button = (label: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label) ?? null;
const warning = () => host.querySelector<HTMLElement>('.ck-changes-keep-warning');

describe('Changes: keeping a branch copy', () => {
  it('asks for the copy of its own pane', async () => {
    await mount(fakeApi());
    expect(lineageMock).toHaveBeenCalledWith('p1');
  });

  it('a normal copy shows no warning in the Merge confirm', async () => {
    await mount(fakeApi());
    await click(button('Merge'));
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Merge into main?');
    expect(warning()).toBeNull();
    expect(host.textContent).not.toContain('This branch started');
  });

  it('an open branch copy shows one plain warning above the Merge button', async () => {
    lineageMock.mockResolvedValue({ from: BRANCH, children: [] });
    const api = fakeApi();
    await mount(api);
    await click(button('Merge'));
    const sheet = host.querySelector('[role="alertdialog"]')!;
    expect(warning()?.textContent).toBe(WARNING);
    expect(warning()?.closest('[role="alertdialog"]')).toBe(sheet);
    const actions = sheet.querySelector('.ck-sheet-actions')!;
    expect(warning()!.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // a warning, never a block: Merge still merges
    expect((Array.from(sheet.querySelectorAll('button')).find((b) => b.textContent === 'Merge') as HTMLButtonElement).disabled).toBe(false);
    await click(Array.from(sheet.querySelectorAll('button')).find((b) => b.textContent === 'Merge') ?? null);
    expect(api.worktreeMerge).toHaveBeenCalledWith('C:/proj', 'run-1');
  });

  it('the Discard confirm of a branch copy does not repeat the warning', async () => {
    lineageMock.mockResolvedValue({ from: BRANCH, children: [] });
    await mount(fakeApi());
    await click(button('Discard'));
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Discard this separate copy?');
    expect(warning()).toBeNull();
  });

  it('a branch that is already kept shows no warning', async () => {
    lineageMock.mockResolvedValue({ from: { ...BRANCH, closed: 'kept' }, children: [] });
    await mount(fakeApi());
    await click(button('Merge'));
    expect(warning()).toBeNull();
  });

  it('a failed lookup shows no warning and still lets Merge run', async () => {
    lineageMock.mockRejectedValue(new Error('no lineage'));
    const api = fakeApi();
    await mount(api);
    await click(button('Merge'));
    expect(warning()).toBeNull();
    await click(Array.from(host.querySelectorAll('.ck-sheet button')).find((b) => b.textContent === 'Merge') ?? null);
    expect(api.worktreeMerge).toHaveBeenCalledWith('C:/proj', 'run-1');
  });
});
