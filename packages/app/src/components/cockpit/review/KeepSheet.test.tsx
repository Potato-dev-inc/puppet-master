// @vitest-environment jsdom
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReviewAct, ReviewItem } from '../../../lib/review-desk';
import { KeepSheet, keepSheetCopy } from './KeepSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ITEM: ReviewItem = {
  key: 'copy:run-1',
  source: 'copy',
  fingerprint: 'fp-1',
  title: 'Make the cart total round to cents',
  agentLabel: 'Codex',
  whereLabel: 'Separate copy of main',
  paneId: 'pane-1',
  finishedAtMs: 0,
  turns: 0,
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
  reasons: [{ flag: 'tests_weakened', sentence: 'The tests were weakened: 2 tests were skipped in cart.test.ts' }],
  keep: { kind: 'merge', label: 'Merge…', into: 'main', why: null },
  throw: { kind: 'discard', why: null },
  fixPaneId: 'pane-1',
  fixWhyNot: null,
};

const ACT: ReviewAct = { sentence: 'Merged into main.', gone: true };

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
const body = () => host.querySelector<HTMLElement>('.ck-rd-sheet-body')!;
const enter = () =>
  act(async () => {
    body().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  });
const buttonWith = (text: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text) ?? null;

describe('keepSheetCopy', () => {
  it('asks Merge into main? for a copy, with the file count', () => {
    const copy = keepSheetCopy(ITEM);
    expect(copy.title).toBe('Merge into main?');
    expect(copy.primary).toBe('Merge');
    expect(copy.body).toBe("The copy's 2 files go into main. The copy is closed after the merge.");
  });

  it('uses the same words for a Night Shift task', () => {
    const copy = keepSheetCopy({ ...ITEM, keep: { kind: 'night_keep', label: 'Merge…', into: 'main', why: null } });
    expect(copy.title).toBe('Merge into main?');
    expect(copy.body).toContain('2 files go into main');
  });

  it('uses the singular for one file', () => {
    const copy = keepSheetCopy({ ...ITEM, files: { ...ITEM.files, total: 1 } });
    expect(copy.body).toBe("The copy's 1 file goes into main. The copy is closed after the merge.");
  });

  it('asks for a draft pull request for an issue run, and never says merge', () => {
    const copy = keepSheetCopy({ ...ITEM, keep: { kind: 'issue_push', label: 'Push as draft…', into: null, why: null } });
    expect(copy.title).toBe('Push as a draft pull request?');
    expect(copy.primary).toBe('Push as draft');
    expect(copy.footer).toBe('Puppet Master opens a draft. It never merges.');
  });
});

describe('KeepSheet', () => {
  it('shows the top reason as Look first when the item is at tier 4 or higher', async () => {
    await render(<KeepSheet item={ITEM} api={{ keep: vi.fn() }} onDone={vi.fn()} onCancel={vi.fn()} />);
    expect(body().textContent).toContain('Look first: The tests were weakened: 2 tests were skipped in cart.test.ts');
  });

  it('shows no Look first line below tier 4', async () => {
    await render(
      <KeepSheet item={{ ...ITEM, tier: 5 }} api={{ keep: vi.fn() }} onDone={vi.fn()} onCancel={vi.fn()} />,
    );
    expect(body().textContent).not.toContain('Look first');
  });

  it('confirms once when Enter is pressed twice', async () => {
    const keep = vi.fn(async () => ACT);
    const onDone = vi.fn();
    await render(<KeepSheet item={ITEM} api={{ keep }} onDone={onDone} onCancel={vi.fn()} />);
    await enter();
    await enter();
    expect(keep).toHaveBeenCalledTimes(1);
    expect(keep).toHaveBeenCalledWith('copy:run-1', 'fp-1');
    expect(onDone).toHaveBeenCalledWith(ACT);
    expect(onDone).toHaveBeenCalledTimes(1);
  });

  it('confirms once when the primary button is pressed', async () => {
    const keep = vi.fn(async () => ACT);
    await render(<KeepSheet item={ITEM} api={{ keep }} onDone={vi.fn()} onCancel={vi.fn()} />);
    await act(async () => {
      buttonWith('Merge')!.click();
    });
    expect(keep).toHaveBeenCalledTimes(1);
  });

  it('shows a conflict as the plain sentence, with the raw code as a tooltip', async () => {
    const raw = 'MERGE_CONFLICT: conflict (src/money.ts)';
    const keep = vi.fn(async () => {
      throw new Error(raw);
    });
    const onDone = vi.fn();
    await render(<KeepSheet item={ITEM} api={{ keep }} onDone={onDone} onCancel={vi.fn()} />);
    await enter();
    const error = host.querySelector<HTMLElement>('.ck-rd-sheet-error')!;
    expect(error.textContent).toContain('Some files changed in both places, so nothing was merged.');
    expect(error.textContent).toContain('(src/money.ts)');
    expect(error.textContent).not.toContain('MERGE_CONFLICT');
    expect(error.title).toBe(raw);
    expect(onDone).not.toHaveBeenCalled();
    expect(keep).toHaveBeenCalledTimes(1);
    expect(buttonWith('Merge')!.disabled).toBe(false);
  });

  it('shows another refusal as its plain message, without the code', async () => {
    const keep = vi.fn(async () => {
      throw new Error('RUN_ACTIVE: The copy is still busy.');
    });
    await render(<KeepSheet item={ITEM} api={{ keep }} onDone={vi.fn()} onCancel={vi.fn()} />);
    await enter();
    expect(host.querySelector('.ck-rd-sheet-error')?.textContent).toBe('The copy is still busy.');
  });

  it('lists the push-check reasons on an issue run, with the footer', async () => {
    const item: ReviewItem = { ...ITEM, keep: { kind: 'issue_push', label: 'Push as draft…', into: null, why: null } };
    await render(
      <KeepSheet
        item={item}
        extra={['A file may hold a secret: .env', 'CI files changed: .github/workflows/ci.yml']}
        api={{ keep: vi.fn() }}
        onDone={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    const lines = [...host.querySelectorAll('.ck-rd-sheet-list li')].map((li) => li.textContent);
    expect(lines).toEqual(['A file may hold a secret: .env', 'CI files changed: .github/workflows/ci.yml']);
    expect(host.querySelector('.ck-rd-sheet-foot')?.textContent).toBe('Puppet Master opens a draft. It never merges.');
    expect(host.querySelector('h2')?.textContent).toBe('Push as a draft pull request?');
  });

  it('Cancel closes without calling Keep', async () => {
    const keep = vi.fn(async () => ACT);
    const onCancel = vi.fn();
    await render(<KeepSheet item={ITEM} api={{ keep }} onDone={vi.fn()} onCancel={onCancel} />);
    await act(async () => {
      buttonWith('Cancel')!.click();
    });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(keep).not.toHaveBeenCalled();
  });
});
