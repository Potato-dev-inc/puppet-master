import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ParsedReview, ReviewStart, SecondOpinionApi } from '../../lib/second-opinion';
import { POLL_MS, beginReview, cancelReview, clearReview, getReviewRun, resetReviews, sendToAuthor } from './second-opinion-store';

const PATCH = 'diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,4 @@\n one\n+two\n three\n-four\n+five\n';

const START: ReviewStart = {
  paneId: 'p1',
  handle: 'h1',
  project: 'C:/proj',
  reviewer: 'codex',
  reviewerLabel: 'Codex',
  source: 'checkpoint',
  sourceLabel: 'the last turn',
  patch: PATCH,
  patchTruncated: false,
  files: 1,
  additions: 2,
  deletions: 1,
  authorLabel: 'Claude Code',
};

const REVIEW: ParsedReview = {
  findings: [{ id: 'f1', file: 'src/a.ts', line: 2, severity: 'high', text: 'Guard the empty case' }],
  verdict: { level: 'concerns', text: 'One gap.' },
  unstructured: null,
};

const REVIEW_OTHER: ParsedReview = { findings: [], verdict: { level: 'approve', text: 'Fine.' }, unstructured: null };

function fakeApi(over: Partial<Record<keyof SecondOpinionApi, unknown>> = {}): SecondOpinionApi {
  return {
    agents: vi.fn().mockResolvedValue([]),
    start: vi.fn().mockResolvedValue(START),
    poll: vi.fn().mockResolvedValue({ state: 'done', review: REVIEW, error: null }),
    cancel: vi.fn().mockResolvedValue(undefined),
    send: vi.fn().mockResolvedValue({ route: 'run', target: 'h1' }),
    ...over,
  } as SecondOpinionApi;
}

/** Let already-resolved promise callbacks run (no timers involved). */
async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
  resetReviews();
});

afterEach(() => {
  resetReviews();
  vi.useRealTimers();
});

describe('second-opinion store', () => {
  it('goes starting, running, then done as the poll answers', async () => {
    const api = fakeApi({
      poll: vi
        .fn()
        .mockResolvedValueOnce({ state: 'running', review: null, error: null })
        .mockResolvedValueOnce({ state: 'done', review: REVIEW, error: null }),
    });

    const started = beginReview('p1', 'codex', api);
    expect(getReviewRun('p1')?.phase).toBe('starting');
    expect(api.start).toHaveBeenCalledWith('p1', 'codex');

    await started;
    expect(getReviewRun('p1')?.phase).toBe('running');
    expect(getReviewRun('p1')?.start).toEqual(START);

    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(api.poll).toHaveBeenCalledTimes(1);
    expect(api.poll).toHaveBeenCalledWith('C:/proj', 'h1');
    expect(getReviewRun('p1')?.phase).toBe('running');

    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(getReviewRun('p1')?.phase).toBe('done');
    expect(getReviewRun('p1')?.review).toEqual(REVIEW);
    expect(getReviewRun('p1')?.reviewer).toBe('codex');
  });

  it('a failing start gives phase failed with the message without its CODE prefix', async () => {
    const api = fakeApi({ start: vi.fn().mockRejectedValue(new Error('NO_AGENT: Codex is not installed')) });

    await beginReview('p1', 'codex', api);

    const run = getReviewRun('p1');
    expect(run?.phase).toBe('failed');
    expect(run?.error).toBe('Codex is not installed');
    expect(run?.start).toBeUndefined();
    expect(api.poll).not.toHaveBeenCalled();
  });

  it('a poll answering state failed gives phase failed with its error', async () => {
    const api = fakeApi({
      poll: vi.fn().mockResolvedValue({ state: 'failed', review: null, error: 'Codex stopped early.' }),
    });

    await beginReview('p1', 'codex', api);
    await vi.advanceTimersByTimeAsync(POLL_MS);

    const run = getReviewRun('p1');
    expect(run?.phase).toBe('failed');
    expect(run?.error).toBe('Codex stopped early.');
    expect(run?.start).toEqual(START);
  });

  it('cancelReview calls api.cancel with the run project and handle, and forgets the run', async () => {
    const api = fakeApi({
      poll: vi.fn().mockResolvedValue({ state: 'running', review: null, error: null }),
    });
    await beginReview('p1', 'codex', api);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(getReviewRun('p1')?.phase).toBe('running');

    await cancelReview('p1', api);

    expect(api.cancel).toHaveBeenCalledWith('C:/proj', 'h1');
    expect(getReviewRun('p1')).toBeUndefined();
    const polls = (api.poll as ReturnType<typeof vi.fn>).mock.calls.length;
    await vi.advanceTimersByTimeAsync(POLL_MS * 3);
    expect(api.poll).toHaveBeenCalledTimes(polls);
  });

  it('clearReview forgets a finished run', async () => {
    const api = fakeApi();
    await beginReview('p1', 'codex', api);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(getReviewRun('p1')?.phase).toBe('done');

    clearReview('p1');

    expect(getReviewRun('p1')).toBeUndefined();
  });

  it('a late poll answer of an earlier review is ignored once a second review began', async () => {
    let answerFirst: (value: unknown) => void = () => undefined;
    const firstPoll = new Promise((resolve) => {
      answerFirst = resolve;
    });
    const api = fakeApi({
      start: vi
        .fn()
        .mockResolvedValueOnce({ ...START, handle: 'hA' })
        .mockResolvedValueOnce({ ...START, handle: 'hB' }),
      poll: vi
        .fn()
        .mockImplementationOnce(() => firstPoll)
        .mockResolvedValueOnce({ state: 'done', review: REVIEW_OTHER, error: null }),
    });

    await beginReview('p1', 'codex', api);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(api.poll).toHaveBeenCalledWith('C:/proj', 'hA');

    await beginReview('p1', 'codex', api);
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(getReviewRun('p1')?.phase).toBe('done');
    expect(getReviewRun('p1')?.start?.handle).toBe('hB');
    expect(getReviewRun('p1')?.review).toEqual(REVIEW_OTHER);

    answerFirst({ state: 'done', review: REVIEW, error: null });
    await flushMicrotasks();

    expect(getReviewRun('p1')?.start?.handle).toBe('hB');
    expect(getReviewRun('p1')?.review).toEqual(REVIEW_OTHER);
  });

  it('sendToAuthor rethrows the error with the CODE prefix stripped', async () => {
    const api = fakeApi({ send: vi.fn().mockRejectedValue(new Error('PANE_GONE: the pane was closed')) });

    await expect(sendToAuthor('p1', 'src/a.ts', 2, 'Guard it', api)).rejects.toThrow(new Error('the pane was closed'));
    expect(api.send).toHaveBeenCalledWith('p1', 'src/a.ts', 2, 'Guard it');
  });
});
