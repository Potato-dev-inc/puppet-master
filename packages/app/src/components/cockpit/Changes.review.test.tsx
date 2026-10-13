// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorktreeDiff, WorktreeItem } from '../../lib/cockpit-api';
import type { ParsedReview, ReviewStart, SecondOpinionApi } from '../../lib/second-opinion';
import { Changes, type ChangesApi } from './Changes';
import { POLL_MS, beginReview, resetReviews } from './second-opinion-store';

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

/** The agent's own code, reviewed in the isolated worktree: line 2 of a.txt is '+agent line'. */
const WORKTREE_START: ReviewStart = {
  paneId: 'p1',
  handle: 'run-1',
  project: 'C:/proj',
  reviewer: 'codex',
  reviewerLabel: 'Codex',
  source: 'worktree',
  sourceLabel: 'the isolated worktree',
  patch: PATCH,
  patchTruncated: false,
  files: 2,
  additions: 1,
  deletions: 1,
  authorLabel: 'Claude Code',
};

const REVIEW: ParsedReview = {
  findings: [{ id: 'f1', file: 'a.txt', line: 2, severity: 'medium', text: 'Guard the agent line' }],
  verdict: { level: 'concerns', text: 'One gap.' },
  unstructured: null,
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

function fakeReviewApi(start: ReviewStart = WORKTREE_START, review: ParsedReview = REVIEW): SecondOpinionApi {
  return {
    agents: vi.fn().mockResolvedValue([]),
    start: vi.fn().mockResolvedValue(start),
    poll: vi.fn().mockResolvedValue({ state: 'done', review, error: null }),
    cancel: vi.fn().mockResolvedValue(undefined),
    send: vi.fn().mockResolvedValue({ route: 'run', target: 'run-1' }),
  };
}

/** Run a review through the real store and let its first poll land. */
async function finishReview(reviewApi: SecondOpinionApi) {
  await beginReview('p1', 'codex', reviewApi);
  await vi.advanceTimersByTimeAsync(POLL_MS);
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  resetReviews();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  resetReviews();
  vi.useRealTimers();
});

async function mount(api: ChangesApi, reviewApi: SecondOpinionApi, status = 'idle') {
  await act(async () => {
    root.render(<Changes paneId="p1" status={status} api={api} reviewApi={reviewApi} />);
  });
  await act(async () => {});
  await act(async () => {});
}

const click = async (el: Element | null) => {
  await act(async () => {
    (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
};
const button = (label: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label) ?? null;
const fileRow = (path: string) => Array.from(host.querySelectorAll('.ck-changes-file')).find((r) => r.querySelector('.ck-changes-path')?.textContent === path) ?? null;

describe('Changes with a second opinion of the worktree', () => {
  it('auto-opens the first file with a note, with the note directly after its diff line', async () => {
    const reviewApi = fakeReviewApi();
    await finishReview(reviewApi);
    await mount(fakeApi(), reviewApi);

    const pre = host.querySelector('pre.ck-diff-annot');
    expect(pre?.getAttribute('aria-label')).toBe('Diff of a.txt');
    const agentLine = Array.from(pre?.querySelectorAll('.ck-diff-line') ?? []).find((l) => l.textContent?.includes('+agent line'));
    const note = host.querySelector('.ck-find');
    expect(note?.textContent).toContain('Guard the agent line');
    expect(agentLine?.nextElementSibling).toBe(note);
  });

  it('the file row shows its note count and a gutter marker sits on the noted line', async () => {
    const reviewApi = fakeReviewApi();
    await finishReview(reviewApi);
    await mount(fakeApi(), reviewApi);

    expect(fileRow('a.txt')?.querySelector('.ck-changes-notes')?.textContent).toBe('1 note');
    const mark = host.querySelector('.ck-gutter--mark');
    expect(mark).not.toBeNull();
    expect(mark?.closest('.ck-diff-line')?.textContent).toContain('+agent line');
  });

  it('Send to Claude Code sends the note through reviewApi with its file, line and text', async () => {
    const reviewApi = fakeReviewApi();
    await finishReview(reviewApi);
    await mount(fakeApi(), reviewApi);

    await click(button('Send to Claude Code'));

    expect(reviewApi.send).toHaveBeenCalledWith('p1', 'a.txt', 2, 'Guard the agent line');
    expect(host.querySelector('.ck-find-send')?.textContent).toBe('Sent');
  });
});

describe('Changes: the review comes before the decision', () => {
  const marker = <section aria-label="Second opinion block">review</section>;

  async function mountWithBlock(api: ChangesApi, reviewApi: SecondOpinionApi) {
    await act(async () => {
      root.render(<Changes paneId="p1" status="idle" api={api} reviewApi={reviewApi} secondOpinion={marker} />);
    });
    await act(async () => {});
    await act(async () => {});
  }

  it('renders the Second opinion block above Merge and Discard', async () => {
    await mountWithBlock(fakeApi(), fakeReviewApi());
    const block = host.querySelector('[aria-label="Second opinion block"]')!;
    const actions = host.querySelector('.ck-changes-actions')!;
    expect(block.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('shows the block on its own when there is no worktree to merge', async () => {
    const api = fakeApi({ paneWorktree: vi.fn().mockResolvedValue(null) });
    await mountWithBlock(api, fakeReviewApi());
    expect(host.querySelector('[aria-label="Second opinion block"]')).not.toBeNull();
    expect(button('Merge')).toBeNull();
  });

  it('a review with concerns turns Merge into a secondary "Merge anyway"', async () => {
    const reviewApi = fakeReviewApi(); // verdict: concerns
    await finishReview(reviewApi);
    await mount(fakeApi(), reviewApi);
    expect(button('Merge')).toBeNull();
    const merge = button('Merge anyway')!;
    expect(merge.className).toContain('ck-btn--secondary');
    expect(merge.className).not.toContain('ck-btn--primary');
    await click(merge);
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Merge into main?');
  });

  it('a clean review (or none) keeps the primary Merge', async () => {
    const approve = fakeReviewApi(WORKTREE_START, { findings: [], verdict: { level: 'approve', text: 'Looks fine.' }, unstructured: null });
    await finishReview(approve);
    await mount(fakeApi(), approve);
    expect(button('Merge')?.className).toContain('ck-btn--primary');
    expect(button('Merge anyway')).toBeNull();
  });
});

describe('Changes without an annotating review', () => {
  it('a checkpoint review does not annotate Changes: the diff stays plain and carries no notes', async () => {
    const reviewApi = fakeReviewApi({ ...WORKTREE_START, source: 'checkpoint', sourceLabel: 'the last turn' });
    await finishReview(reviewApi);
    await mount(fakeApi(), reviewApi);

    expect(host.querySelector('.ck-find')).toBeNull();
    expect(host.querySelector('.ck-changes-notes')).toBeNull();
    expect(host.querySelector('.ck-diff-annot')).toBeNull();

    await click(fileRow('a.txt'));
    const pre = host.querySelector('pre.ck-diff');
    expect(pre?.getAttribute('aria-label')).toBe('Diff of a.txt');
    expect(pre?.querySelector('.ck-find')).toBeNull();
    expect(pre?.querySelector('.ck-gutter--mark')).toBeNull();
    expect(host.querySelector('.ck-diff-annot')).toBeNull();
    expect(host.textContent).toContain('+agent line');
  });

  it('a pane with no review renders the plain <pre class="ck-diff"> diff', async () => {
    const reviewApi = fakeReviewApi();
    await mount(fakeApi(), reviewApi);

    await click(fileRow('a.txt'));

    const pre = host.querySelector('pre');
    expect(pre?.className).toBe('ck-diff');
    expect(pre?.getAttribute('aria-label')).toBe('Diff of a.txt');
    expect(Array.from(pre?.children ?? []).map((c) => c.className)).toEqual([
      'ck-diff-line ck-diff-hunk',
      'ck-diff-line ck-diff-ctx',
      'ck-diff-line ck-diff-add',
    ]);
    expect(host.querySelector('.ck-find')).toBeNull();
    expect(host.querySelector('.ck-changes-notes')).toBeNull();
    expect(reviewApi.start).not.toHaveBeenCalled();
  });
});
