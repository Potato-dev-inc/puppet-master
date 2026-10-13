// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorktreeDiff, WorktreeItem } from '../../lib/cockpit-api';
import { Changes, type ChangesApi } from './Changes';
import { lineKind, splitPatch } from './diff';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATCH = [
  'diff --git a/a.txt b/a.txt',
  'index 111..222 100644',
  '--- a/a.txt',
  '+++ b/a.txt',
  '@@ -1 +1,2 @@',
  ' alpha',
  '+agent line',
  'diff --git a/b.txt b/b.txt',
  'deleted file mode 100644',
  '--- a/b.txt',
  '+++ /dev/null',
  '@@ -1 +0,0 @@',
  '-bravo',
  '',
].join('\n');

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
    files: [
      { path: 'a.txt', status: 'modified', additions: 1, deletions: 0, binary: false },
      { path: 'b.txt', status: 'deleted', additions: 0, deletions: 1, binary: false },
    ],
    files_changed: 2,
    additions: 1,
    deletions: 1,
    truncated: false,
  },
  patch: PATCH,
  patch_truncated: false,
};

function fakeApi(over: Partial<ChangesApi> = {}): ChangesApi {
  return {
    paneWorktree: vi.fn().mockResolvedValue(ITEM),
    worktreeDiff: vi.fn().mockResolvedValue(DIFF),
    worktreeMerge: vi.fn().mockResolvedValue({ merged: true, merge_commit: 'abc', files: [], branch: 'pm/run-1', into: 'main' }),
    worktreeDiscard: vi.fn().mockResolvedValue(undefined),
    ...over,
  } as ChangesApi;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function mount(api: ChangesApi, status = 'idle') {
  await act(async () => {
    root.render(<Changes paneId="p1" status={status} api={api} />);
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

describe('Changes section', () => {
  it('renders nothing for a pane that is not in a worktree', async () => {
    await mount(fakeApi({ paneWorktree: vi.fn().mockResolvedValue(null) }));
    expect(host.querySelector('.ck-changes')).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('shows the branch, totals and files with their +/- counts', async () => {
    const api = fakeApi();
    await mount(api);
    expect(api.worktreeDiff).toHaveBeenCalledWith('C:/proj', 'run-1');
    const text = host.textContent ?? '';
    expect(text).toContain('pm/run-1');
    expect(text).toContain('main');
    expect(text).toContain('2 files');
    expect(text).toContain('+1 −1');
    const rows = host.querySelectorAll('.ck-changes-file');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('a.txt');
    expect(rows[0].textContent).toContain('+1 −0');
    expect(rows[1].querySelector('.ck-changes-mark')?.textContent).toBe('D');
  });

  it('expands a file into a diff styled by weight and rule only (no colour classes)', async () => {
    await mount(fakeApi());
    expect(host.querySelector('.ck-diff')).toBeNull();
    await click(host.querySelector('.ck-changes-file'));
    const lines = Array.from(host.querySelectorAll('.ck-diff-line'));
    expect(lines.map((l) => l.className.replace('ck-diff-line ', ''))).toEqual(['ck-diff-hunk', 'ck-diff-ctx', 'ck-diff-add']);
    expect(lines[2].textContent).toBe('+agent line');
    expect(host.innerHTML).not.toMatch(/red|green|#[0-9a-f]{6}/i);
  });

  it('merge asks for confirmation, then merges and says where', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Merge'));
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Merge into main?');
    expect(api.worktreeMerge).not.toHaveBeenCalled();
    const confirm = Array.from(host.querySelectorAll('.ck-sheet button')).find((b) => b.textContent === 'Merge');
    await click(confirm ?? null);
    expect(api.worktreeMerge).toHaveBeenCalledWith('C:/proj', 'run-1');
    expect(host.textContent).toContain('Merged pm/run-1 into main.');
    expect(host.querySelector('.ck-changes')).toBeNull();
  });

  it('cancel in the sheet changes nothing', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Discard'));
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('cannot be undone');
    await click(button('Cancel'));
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(api.worktreeDiscard).not.toHaveBeenCalled();
  });

  it('discard confirms, then deletes', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Discard'));
    const confirm = Array.from(host.querySelectorAll('.ck-sheet button')).find((b) => b.textContent === 'Discard');
    await click(confirm ?? null);
    expect(api.worktreeDiscard).toHaveBeenCalledWith('C:/proj', 'run-1');
    expect(host.textContent).toContain('Discarded');
  });

  it('a conflict is shown as an alert with the files and the section stays', async () => {
    const api = fakeApi({
      worktreeMerge: vi.fn().mockRejectedValue('MERGE_CONFLICT: 1 file(s) conflict with main (a.txt)'),
    });
    await mount(api);
    await click(button('Merge'));
    await click(Array.from(host.querySelectorAll('.ck-sheet button')).find((b) => b.textContent === 'Merge') ?? null);
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('MERGE_CONFLICT');
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('a.txt');
    expect(host.querySelector('.ck-changes')).not.toBeNull();
  });

  it('disables Merge and Discard while the agent is still working', async () => {
    await mount(fakeApi({ paneWorktree: vi.fn().mockResolvedValue({ ...ITEM, state: 'active' }) }), 'running');
    expect(host.querySelector('.ck-changes-state')?.textContent).toBe('Working');
    expect((button('Merge') as HTMLButtonElement).disabled).toBe(true);
    expect((button('Discard') as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('diff helpers', () => {
  it('splits a patch per file and drops the git headers', () => {
    const files = splitPatch(PATCH);
    expect([...files.keys()]).toEqual(['a.txt', 'b.txt']);
    expect(files.get('a.txt')).toEqual(['@@ -1 +1,2 @@', ' alpha', '+agent line']);
    expect(files.get('b.txt')).toEqual(['@@ -1 +0,0 @@', '-bravo']);
  });
  it('classifies lines', () => {
    expect(['@@ x', '+a', '-b', ' c'].map(lineKind)).toEqual(['hunk', 'add', 'del', 'ctx']);
  });
});
