// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChangeBlame, WorktreeDiff, WorktreeItem } from '../../lib/cockpit-api';
import { Changes, type ChangesApi } from './Changes';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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
      { path: 'b.txt', status: 'modified', additions: 1, deletions: 1, binary: false },
    ],
    files_changed: 2,
    additions: 2,
    deletions: 1,
    truncated: false,
  },
  patch: '',
  patch_truncated: false,
};

const AT = new Date(2026, 9, 28, 14, 2).getTime();

function fakeApi(blame?: Record<string, ChangeBlame> | 'missing' | 'fails'): ChangesApi {
  const api = {
    paneWorktree: vi.fn().mockResolvedValue(ITEM),
    worktreeDiff: vi.fn().mockResolvedValue(DIFF),
    worktreeMerge: vi.fn(),
    worktreeDiscard: vi.fn(),
  } as unknown as ChangesApi;
  if (blame === 'fails') api.changesBlame = vi.fn().mockRejectedValue(new Error('no folder'));
  else if (blame !== 'missing') api.changesBlame = vi.fn().mockResolvedValue(blame ?? {});
  return api;
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

async function mount(api: ChangesApi) {
  await act(async () => {
    root.render(<Changes paneId="p1" status="idle" api={api} />);
  });
  await act(async () => {});
}

/** The file row of `path`, found by its visible name. */
const row = (path: string) =>
  Array.from(host.querySelectorAll('.ck-changes-file')).find(
    (b) => b.querySelector('.ck-changes-path')?.textContent === path,
  ) as HTMLElement | undefined;

describe('Changes: agent blame line', () => {
  it('shows "Codex helper · 14:02" next to the file that the agent changed, with its hover text', async () => {
    const api = fakeApi({ 'a.txt': { agent: 'Codex', paneTitle: 'helper', atMs: AT } });
    await mount(api);
    expect(api.changesBlame).toHaveBeenCalledWith(ITEM.path, ['a.txt', 'b.txt']);
    const line = row('a.txt')!.querySelector<HTMLElement>('.ck-changes-blame')!;
    expect(line.textContent).toBe('Codex helper \u00b7 14:02');
    expect(line.title).toBe('Changed by Codex in turn at 14:02');
  });

  it('shows no line for a file no turn touched (never a guess)', async () => {
    await mount(fakeApi({ 'a.txt': { agent: 'Codex', paneTitle: null, atMs: AT } }));
    expect(row('b.txt')!.querySelector('.ck-changes-blame')).toBeNull();
    expect(host.querySelectorAll('.ck-changes-blame')).toHaveLength(1);
  });

  it('shows no line at all when nothing is known', async () => {
    await mount(fakeApi({}));
    expect(host.querySelector('.ck-changes-blame')).toBeNull();
    expect(host.querySelectorAll('.ck-changes-file')).toHaveLength(2);
  });

  it('shows no line for an empty answer', async () => {
    const api = fakeApi();
    api.changesBlame = vi.fn().mockResolvedValue(null);
    await mount(api);
    expect(host.querySelector('.ck-changes-blame')).toBeNull();
    expect(host.querySelectorAll('.ck-changes-file')).toHaveLength(2);
  });

  it('shows no line when the lookup fails or the API has no blame call', async () => {
    await mount(fakeApi('fails'));
    expect(host.querySelector('.ck-changes-blame')).toBeNull();
    expect(host.querySelectorAll('.ck-changes-file')).toHaveLength(2);

    await mount(fakeApi('missing'));
    expect(host.querySelector('.ck-changes-blame')).toBeNull();
    expect(host.querySelectorAll('.ck-changes-file')).toHaveLength(2);
  });
});
