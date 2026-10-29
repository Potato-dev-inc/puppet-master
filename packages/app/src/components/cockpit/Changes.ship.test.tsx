// @vitest-environment jsdom
// Changes tab, shipping: Create PR, the PR block with its checks, how the copy was set up.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorktreeDiff, WorktreeItem } from '../../lib/cockpit-api';
import type { ChecksReport, PrPrepared, PrRef, SetupReport, ShipApi } from '../../lib/ship-api';
import { Changes, type ChangesApi } from './Changes';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const ITEM: WorktreeItem = {
  handle: 'run-1',
  branch: 'pm/run-1',
  base: 'main',
  repo: 'C:/proj',
  path: 'C:/proj/.puppet-master/worktrees/run-1-ab12',
  exists: true,
  state: 'unreviewed',
  summary: null,
  agent_type: 'codex',
};

const DIFF: WorktreeDiff = {
  branch: 'pm/run-1',
  base: 'main',
  changes: { files: [{ path: 'a.txt', status: 'modified', additions: 1, deletions: 0, binary: false }], files_changed: 1, additions: 1, deletions: 0, truncated: false },
  patch: '',
  patch_truncated: false,
};

const PREPARED: PrPrepared = {
  gh: 'ready', has_remote: true, on_github: true, default_branch: 'main', existing: null,
  title: 'Fix cart', body: '## What changed', base: 'main', branch: 'pm/run-1', agent: 'Codex', has_uncommitted: false,
};

const failing: ChecksReport = { state: 'failing', checked_at_ms: 1, items: [{ name: 'test (ubuntu)', state: 'fail' }, { name: 'lint', state: 'fail' }] };
const PR: PrRef = { number: 42, url: 'https://github.com/o/r/pull/42', draft: true, base: 'main', created_at_ms: 1, checks: failing };

function changesApi(item: WorktreeItem): ChangesApi {
  return {
    paneWorktree: vi.fn().mockResolvedValue(item),
    worktreeDiff: vi.fn().mockResolvedValue(DIFF),
    worktreeMerge: vi.fn(),
    worktreeDiscard: vi.fn(),
  } as unknown as ChangesApi;
}

function ship(over: Partial<ShipApi> = {}): ShipApi {
  return {
    prPrepare: vi.fn().mockResolvedValue(PREPARED),
    prCreate: vi.fn().mockResolvedValue({ ...PR, checks: undefined }),
    prChecks: vi.fn().mockResolvedValue({ ...PR, checks: { state: 'pending', checked_at_ms: 2, items: [{ name: 'test', state: 'pending' }] } }),
    openUrl: vi.fn(),
    ...over,
  } as unknown as ShipApi;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (label: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;

async function mount(item: WorktreeItem, api: ShipApi, extra: { hideActions?: boolean } = {}) {
  await act(async () => {
    root.render(<Changes paneId="p1" status="idle" api={changesApi(item)} ship={api} {...extra} />);
  });
  await act(async () => {});
}

describe('Create PR on the Changes tab', () => {
  it('sits between Merge and Discard', async () => {
    await mount(ITEM, ship());
    const labels = Array.from(host.querySelectorAll('.ck-changes-actions button')).map((b) => b.textContent);
    expect(labels).toEqual(['Merge', 'Create PR', 'Discard']);
  });

  it('opens the sheet, creates the draft and shows the PR block with its link', async () => {
    const api = ship();
    await mount(ITEM, api);
    await act(async () => button('Create PR')?.click());
    await act(async () => {});
    expect(document.body.textContent).toContain('Create a draft pull request');
    await act(async () => button('Create draft PR')?.click());
    await act(async () => {});
    expect(api.prCreate).toHaveBeenCalledWith('C:/proj', 'run-1', 'Fix cart', '## What changed', 'main');
    expect(host.textContent).toContain('Draft PR #42');
    expect(host.textContent).toContain('Draft pull request #42 created.');
    expect(button('Update PR')).toBeTruthy();
    expect(button('Create PR')).toBeUndefined();
  });

  it('is hidden when another surface owns the decisions', async () => {
    await mount(ITEM, ship(), { hideActions: true });
    expect(button('Create PR')).toBeUndefined();
  });

  it('cannot be used while the agent is still working', async () => {
    await mount({ ...ITEM, state: 'active' }, ship());
    expect(button('Create PR')?.disabled).toBe(true);
  });
});

describe('a copy that already has a PR', () => {
  it('shows the failed GitHub checks and Fix with the agent that wrote it', async () => {
    await mount({ ...ITEM, pr: PR }, ship());
    expect(host.textContent).toContain('Draft PR #42');
    expect(host.textContent).toContain('2 GitHub checks failed: test (ubuntu), lint');
    const fix = Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.startsWith('Fix with '));
    expect(fix).toBeTruthy();
    expect(host.textContent).not.toContain('Send back');
    expect(button('Update PR')).toBeTruthy();
  });
});

describe('how the copy was set up', () => {
  const setup = (status: 'ok' | 'failed', tail = ''): SetupReport => ({
    status,
    copied: status === 'ok' ? ['.env'] : [],
    not_copied: [],
    steps: [{ command: 'npm ci', ok: status === 'ok', exit_code: status === 'ok' ? 0 : 1, timed_out: false, tail, duration_ms: 1 }],
    duration_ms: 1,
  });

  it('says what ran and what was copied', async () => {
    await mount({ ...ITEM, setup: setup('ok') }, ship());
    expect(host.querySelector('.ck-changes-setup')?.textContent).toContain('Ran npm ci; copied .env (never committed).');
  });

  it('says a failed setup in plain words and keeps the output one click away', async () => {
    await mount({ ...ITEM, setup: setup('failed', 'npm ERR! missing script') }, ship());
    const block = host.querySelector('.ck-changes-setup');
    expect(block?.textContent).toContain('Setting up the copy failed: npm ci stopped with an error.');
    expect(block?.querySelector('[role="alert"]')).toBeTruthy();
    expect(block?.querySelector('pre')?.textContent).toContain('npm ERR! missing script');
  });

  it('shows nothing for a copy made without a preset', async () => {
    await mount(ITEM, ship());
    expect(host.querySelector('.ck-changes-setup')).toBeNull();
  });
});
