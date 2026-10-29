// @vitest-environment jsdom
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReviewAct, ReviewDetail as ReviewDetailView, ReviewItem } from '../../../lib/review-desk';
import { LOOKED_TEXT, NO_PANE_TEXT, ReviewDetail, countsText, type ReviewDetailApi } from './ReviewDetail';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PATCH = [
  'diff --git a/src/money.ts b/src/money.ts',
  'index 1111..2222 100644',
  '--- a/src/money.ts',
  '+++ b/src/money.ts',
  '@@ -10,3 +10,3 @@',
  ' const rate = 1;',
  '-old line',
  '+new <script>alert(1)</script>',
  'diff --git a/test/cart.test.ts b/test/cart.test.ts',
  'index 3333..4444 100644',
  '--- a/test/cart.test.ts',
  '+++ b/test/cart.test.ts',
  '@@ -1 +1 @@',
  '-it("adds")',
  '+it.skip("adds")',
  '',
].join('\n');

const ITEM: ReviewItem = {
  key: 'copy:run-1',
  source: 'copy',
  fingerprint: 'fp-1',
  title: 'Make the cart total round to cents',
  agentLabel: 'Codex',
  whereLabel: 'Separate copy of main',
  paneId: 'pane-1',
  finishedAtMs: new Date(2026, 9, 29, 8, 14).getTime(),
  turns: 3,
  group: null,
  files: {
    total: 2,
    added: 52,
    deleted: 20,
    rows: [
      { path: 'src/money.ts', status: 'M', added: 40, deleted: 12, binary: false },
      { path: 'test/cart.test.ts', status: 'M', added: 12, deleted: 8, binary: false },
    ],
  },
  level: 'high',
  levelWord: 'High risk',
  tier: 1,
  reasons: [
    { flag: 'tests_weakened', sentence: 'The tests were weakened: 2 tests were skipped in cart.test.ts' },
    { flag: 'tests_changed', sentence: 'Test files changed: cart.test.ts' },
  ],
  keep: { kind: 'merge', label: 'Merge…', into: 'main', why: null },
  throw: { kind: 'discard', why: null },
  fixPaneId: 'pane-1',
  fixWhyNot: null,
};

const DETAIL: ReviewDetailView = { item: ITEM, patch: PATCH, patchTruncated: false, extra: [] };

function fakeApi(over: { detail?: () => Promise<ReviewDetailView>; keep?: () => Promise<ReviewAct> } = {}) {
  return {
    detail: vi.fn(over.detail ?? (async (_key: string) => DETAIL)),
    keep: vi.fn(over.keep ?? (async (_key: string, _fingerprint: string) => ({ sentence: 'Merged.', gone: true }))),
    fix: vi.fn(async (_key: string, _fingerprint: string, _note: string) => 'pane-1'),
  } satisfies ReviewDetailApi & Record<string, unknown>;
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

async function render(el: ReactElement) {
  await act(async () => {
    root.render(el);
  });
}
const buttonWith = (text: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(text)) ?? null;
async function press(el: HTMLElement) {
  await act(async () => {
    el.click();
  });
}

describe('ReviewDetail: header, reasons and facts', () => {
  it('shows the title, the level, the reasons and the facts, and loads the item detail', async () => {
    const api = fakeApi();
    await render(<ReviewDetail item={ITEM} api={api} />);
    expect(api.detail).toHaveBeenCalledWith('copy:run-1');
    expect(host.querySelector('.ck-rd-detail-title')?.textContent).toBe(ITEM.title);
    expect(host.querySelector('.ck-rd-detail-level')?.textContent).toContain('High risk');
    const reasons = host.querySelector('.ck-rd-detail-reasons')?.textContent ?? '';
    expect(reasons).toContain('The tests were weakened: 2 tests were skipped in cart.test.ts');
    expect(reasons).toContain('Test files changed: cart.test.ts');
    expect(host.querySelector('.ck-rd-detail-meta')?.textContent).toContain('Codex');
    expect(host.querySelector('.ck-rd-detail-facts')?.textContent).toContain('2 files');
  });

  it('says No flags when the item has no reasons', async () => {
    await render(<ReviewDetail item={{ ...ITEM, reasons: [], tier: 7, level: 'low', levelWord: 'Low risk' }} api={fakeApi()} />);
    expect(host.querySelector('.ck-rd-detail-reasons')?.textContent).toBe('No flags');
  });
});

describe('ReviewDetail: files and the diff', () => {
  it('shows the diff of the first file as text, so a script tag never becomes markup', async () => {
    await render(<ReviewDetail item={ITEM} api={fakeApi()} />);
    await act(async () => {});
    const diff = host.querySelector('.ck-rd-detail-diff');
    expect(diff).not.toBeNull();
    expect(diff?.querySelector('script')).toBeNull();
    expect(host.querySelector('script')).toBeNull();
    expect(diff?.textContent).toContain('+new <script>alert(1)</script>');
    expect(diff?.querySelector('.ck-rd-detail-add')?.textContent).toBe('+new <script>alert(1)</script>');
    expect(diff?.querySelector('.ck-rd-detail-del')?.textContent).toBe('-old line');
    expect(diff?.querySelector('.ck-rd-detail-hunk')?.textContent).toBe('@@ -10,3 +10,3 @@');
  });

  it('switches the diff to another file when its row is pressed', async () => {
    await render(<ReviewDetail item={ITEM} api={fakeApi()} />);
    await act(async () => {});
    const rows = host.querySelectorAll<HTMLButtonElement>('.ck-rd-detail-file');
    expect(rows[0].getAttribute('aria-pressed')).toBe('true');
    await press(rows[1]);
    expect(rows[1].getAttribute('aria-pressed')).toBe('true');
    const diff = host.querySelector('.ck-rd-detail-diff');
    expect(diff?.textContent).toContain('+it.skip("adds")');
    expect(diff?.textContent).not.toContain('<script>');
  });

  it('shows the counts with a real minus sign', () => {
    expect(countsText(52, 20)).toBe('+52 −20');
    expect(countsText(1204, 88)).toBe('+1,204 −88');
  });

  it('says cut when the patch was truncated', async () => {
    await render(<ReviewDetail item={ITEM} api={fakeApi({ detail: async () => ({ ...DETAIL, patchTruncated: true }) })} />);
    await act(async () => {});
    expect(host.textContent).toContain('The diff is cut.');
  });
});

describe('ReviewDetail: actions', () => {
  it('Keep opens the merge sheet and does not merge by itself', async () => {
    const api = fakeApi();
    await render(<ReviewDetail item={{ ...ITEM, tier: 1 }} api={api} />);
    await act(async () => {});
    await press(buttonWith('Merge…')!);
    expect(host.querySelector('.ck-sheet h2')?.textContent).toBe('Merge into main?');
    expect(host.textContent).toContain('Look first: The tests were weakened');
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('hands Keep to the desk when onKeep is given, and opens no sheet', async () => {
    const api = fakeApi();
    const onKeep = vi.fn();
    await render(<ReviewDetail item={ITEM} api={api} onKeep={onKeep} />);
    await act(async () => {});
    await press(buttonWith('Merge…')!);
    expect(onKeep).toHaveBeenCalledWith(ITEM);
    expect(host.querySelector('.ck-sheet')).toBeNull();
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('Looks good marks the in-folder turn at once and reports it', async () => {
    const api = fakeApi();
    const onKept = vi.fn();
    const item: ReviewItem = { ...ITEM, source: 'folder', key: 'folder:pane-1:1-2', keep: { kind: 'mark', label: 'Looks good', into: null, why: null } };
    await render(<ReviewDetail item={item} api={api} onKept={onKept} />);
    await act(async () => {});
    await press(buttonWith('Looks good')!);
    expect(api.keep).toHaveBeenCalledWith('folder:pane-1:1-2', 'fp-1');
    expect(host.querySelector('.ck-sheet')).toBeNull();
    expect(host.textContent).toContain(LOOKED_TEXT);
    expect(onKept).toHaveBeenCalledWith({ sentence: 'Merged.', gone: true }, item);
  });

  it('a Keep that is off is disabled and shows its why as a tooltip and a muted line', async () => {
    const why = 'Held. Open Settings > Assign an issue.';
    await render(<ReviewDetail item={{ ...ITEM, keep: { kind: 'no', label: 'Merge…', why } }} api={fakeApi()} />);
    const keep = buttonWith('Merge…')!;
    expect(keep.disabled).toBe(true);
    expect(keep.title).toBe(why);
    expect(host.textContent).toContain(why);
  });

  it('a disabled Throw away shows why', async () => {
    const why = 'This copy is still running.';
    await render(<ReviewDetail item={{ ...ITEM, throw: { kind: 'no', why } }} api={fakeApi()} onThrow={vi.fn()} />);
    const throwButton = buttonWith('Throw away')!;
    expect(throwButton.disabled).toBe(true);
    expect(throwButton.title).toBe(why);
    expect(host.textContent).toContain(why);
  });

  it('Throw away calls onThrow with the item (the desk runs the Undo window)', async () => {
    const onThrow = vi.fn();
    await render(<ReviewDetail item={ITEM} api={fakeApi()} onThrow={onThrow} />);
    await press(buttonWith('Throw away')!);
    expect(onThrow).toHaveBeenCalledWith(ITEM);
  });

  it('Open pane opens the row pane, and is off when the pane is closed', async () => {
    const onOpenPane = vi.fn();
    await render(<ReviewDetail item={ITEM} api={fakeApi()} onOpenPane={onOpenPane} />);
    await press(buttonWith('Open pane')!);
    expect(onOpenPane).toHaveBeenCalledWith('pane-1');

    await render(
      <ReviewDetail item={{ ...ITEM, paneId: null, fixPaneId: null, fixWhyNot: NO_PANE_TEXT }} api={fakeApi()} onOpenPane={onOpenPane} />,
    );
    const open = buttonWith('Open pane')!;
    expect(open.disabled).toBe(true);
    expect(open.title).toBe(NO_PANE_TEXT);
    expect(buttonWith('Ask to fix')!.disabled).toBe(true);
  });

  it('shows a plain line when the item changed since the list was made', async () => {
    await render(
      <ReviewDetail
        item={ITEM}
        api={fakeApi({ detail: async () => ({ ...DETAIL, item: { ...ITEM, fingerprint: 'fp-2' } }) })}
      />,
    );
    await act(async () => {});
    expect(host.textContent).toContain('This changed since you looked. Look again.');
  });
});
