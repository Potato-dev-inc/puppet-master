// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DiffSource, ParsedReview, ReviewerEntry, ReviewStart, SecondOpinionApi } from '../../lib/second-opinion';
import { SecondOpinion } from './SecondOpinion';
import { POLL_MS, beginReview, resetReviews } from './second-opinion-store';
import type { ReviewFinding } from './review-diff';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATCH =
  'diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,4 @@\n one\n+two\n three\n-four\n+five\n';

const AGENTS: ReviewerEntry[] = [
  { id: 'claude', label: 'Claude Code', ready: false, same: true },
  { id: 'codex', label: 'Codex', ready: true, same: false },
  { id: 'gemini', label: 'Gemini', ready: false, same: false },
];

function startOf(source: DiffSource, sourceLabel: string): ReviewStart {
  return {
    paneId: 'p1',
    handle: 'h1',
    project: 'C:/proj',
    reviewer: 'codex',
    reviewerLabel: 'Codex',
    source,
    sourceLabel,
    patch: PATCH,
    patchTruncated: false,
    files: 1,
    additions: 2,
    deletions: 1,
    authorLabel: 'Claude Code',
  };
}

/** Line 2 of the new file is '+two'. */
const ON_LINE_TWO: ReviewFinding = { id: 'f1', file: 'src/a.ts', line: 2, severity: 'high', text: 'Guard the empty case' };

function reviewOf(findings: ReviewFinding[], over: Partial<ParsedReview> = {}): ParsedReview {
  return { findings, verdict: { level: 'concerns', text: 'One gap in the parser.' }, unstructured: null, ...over };
}

function fakeApi(over: Partial<SecondOpinionApi> = {}): SecondOpinionApi {
  return {
    agents: vi.fn().mockResolvedValue(AGENTS),
    start: vi.fn().mockResolvedValue(startOf('checkpoint', 'the last turn')),
    poll: vi.fn().mockResolvedValue({ state: 'done', review: reviewOf([ON_LINE_TWO]), error: null }),
    cancel: vi.fn().mockResolvedValue(undefined),
    send: vi.fn().mockResolvedValue({ route: 'run', target: 'h1' }),
    ...over,
  };
}

/** Start a review through the real store and let its first poll land. */
async function finishReview(api: SecondOpinionApi) {
  await beginReview('p1', 'codex', api);
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

async function mount(api: SecondOpinionApi, status = 'idle') {
  await act(async () => {
    root.render(<SecondOpinion paneId="p1" status={status} api={api} />);
  });
  await act(async () => {});
}

const click = async (el: Element | null) => {
  await act(async () => {
    (el as HTMLElement).dispatchEvent(new MouseEvent('click', { bubbles: true }));
  });
};
const clickNative = async (el: Element | null) => {
  await act(async () => {
    (el as HTMLElement).click();
  });
};
const button = (label: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === label) ?? null;
const sheet = () => host.querySelector('[role="dialog"]');
const sheetRow = (name: string) => Array.from(host.querySelectorAll('.ck-so-row')).find((r) => r.querySelector('.ck-so-name')?.textContent === name) ?? null;

describe('SecondOpinion reviewer sheet', () => {
  it('lists the reviewers, hides the author agent by default and preselects the ready one', async () => {
    await mount(fakeApi());
    await click(button('Second opinion'));

    expect(sheet()?.querySelector('h2')?.textContent).toBe('Who should review?');
    const names = Array.from(host.querySelectorAll('.ck-so-name')).map((n) => n.textContent);
    expect(names).toEqual(['Codex', 'Gemini']);
    expect(sheetRow('Claude Code')).toBeNull();
    expect(sheetRow('Codex')?.querySelector('.ck-so-tag')?.textContent).toBe('ready');
    const radios = Array.from(host.querySelectorAll<HTMLInputElement>('input[type="radio"]'));
    expect(radios.find((r) => r.value === 'codex')?.checked).toBe(true);
  });

  it('ticking "Also allow" lists the author agent tagged "wrote this"', async () => {
    await mount(fakeApi());
    await click(button('Second opinion'));

    const allow = host.querySelector('.ck-so-same');
    expect(allow?.textContent).toContain('Also allow Claude Code, the agent that wrote this');
    await clickNative(allow?.querySelector('input[type="checkbox"]') ?? null);

    expect(sheetRow('Claude Code')?.querySelector('.ck-so-tag')?.textContent).toBe('wrote this');
    expect(sheetRow('Codex')).not.toBeNull();
  });

  it('Start review calls api.start with the preselected reviewer and closes the sheet', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Second opinion'));

    await click(button('Start review'));

    expect(api.start).toHaveBeenCalledWith('p1', 'codex');
    expect(sheet()).toBeNull();
  });

  it('Start review sends the reviewer the user picked, including the author agent once allowed', async () => {
    const api = fakeApi();
    await mount(api);
    await click(button('Second opinion'));
    await clickNative(host.querySelector('.ck-so-same input[type="checkbox"]'));
    const radios = Array.from(host.querySelectorAll<HTMLInputElement>('input[type="radio"]'));
    await clickNative(radios.find((r) => r.value === 'claude') ?? null);

    await click(button('Start review'));

    expect(api.start).toHaveBeenCalledWith('p1', 'claude');
  });
});

describe('SecondOpinion while a review runs', () => {
  it('the Second opinion button is disabled while the agent is still working', async () => {
    await mount(fakeApi(), 'running');
    expect((button('Second opinion') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a working spinner, the reviewer and source, and Stop cancels the run', async () => {
    const api = fakeApi({ poll: vi.fn().mockResolvedValue({ state: 'running', review: null, error: null }) });
    await finishReview(api);
    await mount(api);

    const busy = host.querySelector('.ck-so-busy');
    expect(busy?.querySelector('[role="img"][aria-label="Working"]')).not.toBeNull();
    expect(busy?.textContent).toContain('Codex is reviewing the last turn…');

    await click(button('Stop'));

    expect(api.cancel).toHaveBeenCalledWith('C:/proj', 'h1');
    expect(host.querySelector('.ck-so-busy')).toBeNull();
  });
});

describe('SecondOpinion finished review', () => {
  it('a checkpoint review shows the verdict, meta line, a file row with its note, and the annotated diff open', async () => {
    const api = fakeApi();
    await finishReview(api);
    await mount(api);

    const verdict = host.querySelector('.ck-so-verdict');
    expect(verdict?.getAttribute('data-level')).toBe('concerns');
    expect(verdict?.textContent).toContain('Some concerns.');
    expect(verdict?.textContent).toContain('One gap in the parser.');
    expect(host.textContent).toContain('Codex reviewed the last turn · 1 file · +2 −1');

    const row = host.querySelector('.ck-changes-file');
    expect(row?.textContent).toContain('src/a.ts');
    expect(row?.textContent).toContain('1 note');

    const pre = host.querySelector('pre[aria-label="Diff of src/a.ts"]');
    expect(pre).not.toBeNull();
    const twoLine = Array.from(pre?.querySelectorAll('.ck-diff-line') ?? []).find((l) => l.textContent?.includes('+two'));
    expect(twoLine?.nextElementSibling?.classList.contains('ck-find')).toBe(true);
    expect(twoLine?.nextElementSibling?.textContent).toContain('Guard the empty case');
  });

  it('clicking the file row closes the annotated diff and opens it again', async () => {
    const api = fakeApi();
    await finishReview(api);
    await mount(api);
    expect(host.querySelector('pre[aria-label="Diff of src/a.ts"]')).not.toBeNull();

    await click(host.querySelector('.ck-changes-file'));
    expect(host.querySelector('pre[aria-label="Diff of src/a.ts"]')).toBeNull();

    await click(host.querySelector('.ck-changes-file'));
    expect(host.querySelector('pre[aria-label="Diff of src/a.ts"]')).not.toBeNull();
  });

  it('Send to Claude Code sends the note to the author with its file and line, then reads Sent', async () => {
    const api = fakeApi();
    await finishReview(api);
    await mount(api);

    const send = host.querySelector('.ck-find-send');
    expect(send?.textContent).toBe('Send to Claude Code');
    await click(send);

    expect(api.send).toHaveBeenCalledWith('p1', 'src/a.ts', 2, 'Guard the empty case');
    expect(host.querySelector('.ck-find-send')?.textContent).toBe('Sent');
  });

  it('an unanchored finding is listed under Notes with file:line and a Send button', async () => {
    const elsewhere: ReviewFinding = { id: 'f9', file: 'src/a.ts', line: 99, severity: 'low', text: 'Check the range' };
    const api = fakeApi({ poll: vi.fn().mockResolvedValue({ state: 'done', review: reviewOf([elsewhere]), error: null }) });
    await finishReview(api);
    await mount(api);

    expect(host.querySelector('.ck-caption')?.textContent).toBe('Notes');
    const item = host.querySelector('.ck-so-find');
    expect(item?.querySelector('.ck-so-place')?.textContent).toBe('src/a.ts:99');
    expect(item?.textContent).toContain('Check the range');
    expect(host.querySelector('pre[aria-label="Diff of src/a.ts"]')).toBeNull();

    const send = item?.querySelector('button') ?? null;
    expect(send?.textContent).toBe('Send to Claude Code');
    await click(send);
    expect(api.send).toHaveBeenCalledWith('p1', 'src/a.ts', 99, 'Check the range');
  });

  it('a worktree review only says the notes are pinned in Changes, with no annotated diff here', async () => {
    const api = fakeApi({ start: vi.fn().mockResolvedValue(startOf('worktree', 'the isolated worktree')) });
    await finishReview(api);
    await mount(api);

    expect(host.textContent).toContain('1 note pinned to lines in Changes above.');
    expect(host.querySelector('.ck-diff')).toBeNull();
    expect(host.querySelector('.ck-diff-annot')).toBeNull();
    expect(host.querySelector('.ck-find')).toBeNull();
    expect(host.querySelector('.ck-changes-files')).toBeNull();
  });

  it('a review that failed shows the error as an alert and offers Try another agent', async () => {
    const api = fakeApi({
      poll: vi.fn().mockResolvedValue({ state: 'failed', review: null, error: 'Codex is not signed in.' }),
    });
    await finishReview(api);
    await mount(api);

    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Codex is not signed in.');
    expect(button('Try another agent')).not.toBeNull();
  });

  it('an unstructured answer says the reviewer did not use the expected format and shows its words', async () => {
    const api = fakeApi({
      poll: vi.fn().mockResolvedValue({
        state: 'done',
        review: { findings: [], verdict: null, unstructured: 'Looks fine to me, I think.' },
        error: null,
      }),
    });
    await finishReview(api);
    await mount(api);

    expect(host.textContent).toContain('Codex did not use the expected format.');
    expect(host.querySelector('.ck-so-raw')?.textContent).toBe('Looks fine to me, I think.');
  });
});
