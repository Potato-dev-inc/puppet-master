// @vitest-environment jsdom
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReviewItem } from '../../../lib/review-desk';
import { FIX_NAMES_SHOWN, FIX_NOTE_MAX, FixSheet, fixPreviewLines } from './FixSheet';

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
  reasons: [
    { flag: 'tests_weakened', sentence: 'The tests were weakened: 2 tests were skipped in cart.test.ts' },
    { flag: 'tests_changed', sentence: 'Test files changed: cart.test.ts' },
  ],
  keep: { kind: 'merge', label: 'Merge…', into: 'main', why: null },
  throw: { kind: 'discard', why: null },
  fixPaneId: 'pane-1',
  fixWhyNot: null,
};

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
const textarea = () => host.querySelector<HTMLTextAreaElement>('textarea')!;
const buttonWith = (text: string) =>
  [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text) ?? null;

/** Types into a React-controlled textarea (sets the value the way the browser does, then fires input). */
function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  act(() => {
    setter.call(textarea(), text);
    textarea().dispatchEvent(new Event('input', { bubbles: true }));
  });
}

describe('fixPreviewLines', () => {
  it('holds the reasons, the file names and no note when the note is empty', () => {
    expect(fixPreviewLines(ITEM, '')).toEqual([
      '- The tests were weakened: 2 tests were skipped in cart.test.ts',
      '- Test files changed: cart.test.ts',
      'Files: src/money.ts, test/cart.test.ts',
    ]);
  });

  it('adds the note as one line, trimmed', () => {
    const lines = fixPreviewLines(ITEM, '  Check the rounding.  ');
    expect(lines[lines.length - 1]).toBe('Note: Check the rounding.');
  });

  it('shows at most three file names and says how many more', () => {
    const rows = Array.from({ length: 5 }, (_, i) => ({ path: `f${i}.ts`, status: 'M' as const, added: 1, deleted: 0, binary: false }));
    const lines = fixPreviewLines({ ...ITEM, files: { total: 5, added: 5, deleted: 0, rows } }, '');
    expect(FIX_NAMES_SHOWN).toBe(3);
    expect(lines[lines.length - 1]).toBe('Files: f0.ts, f1.ts, f2.ts and 2 more');
  });
});

describe('FixSheet', () => {
  it('shows the preview, a note box that holds 1,000 characters, and says nothing is sent', async () => {
    await render(<FixSheet item={ITEM} api={{ fix: vi.fn() }} onDone={vi.fn()} onCancel={vi.fn()} />);
    expect(host.querySelector('.ck-rd-fix-preview')?.textContent).toContain('- Test files changed: cart.test.ts');
    expect(textarea().maxLength).toBe(FIX_NOTE_MAX);
    expect(host.querySelector('.ck-rd-sheet-foot')?.textContent).toBe('It is not sent. You can edit it in the chat box.');
    expect(host.textContent).toContain('Add a note (optional)');
  });

  it('Put in chat sends the fixed fields and the note, then reports the pane', async () => {
    const fix = vi.fn(async (_key: string, _fingerprint: string, _note: string) => 'pane-9');
    const onDone = vi.fn();
    await render(<FixSheet item={ITEM} api={{ fix }} onDone={onDone} onCancel={vi.fn()} />);
    type('Check the rounding');
    expect(host.querySelector('.ck-rd-fix-preview')?.textContent).toContain('Note: Check the rounding');
    await act(async () => {
      buttonWith('Put in chat')!.click();
    });
    expect(fix).toHaveBeenCalledWith('copy:run-1', 'fp-1', 'Check the rounding');
    expect(onDone).toHaveBeenCalledWith('pane-9');
  });

  it('Enter puts the draft in once; Shift+Enter does not', async () => {
    const fix = vi.fn(async (_key: string, _fingerprint: string, _note: string) => 'pane-1');
    await render(<FixSheet item={ITEM} api={{ fix }} onDone={vi.fn()} onCancel={vi.fn()} />);
    const area = textarea();
    act(() => {
      area.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true }));
    });
    expect(fix).not.toHaveBeenCalled();
    await act(async () => {
      area.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    });
    await act(async () => {
      area.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    });
    expect(fix).toHaveBeenCalledTimes(1);
    expect(fix).toHaveBeenCalledWith('copy:run-1', 'fp-1', '');
  });

  it('shows the plain refusal and keeps the sheet open', async () => {
    const fix = vi.fn(async () => {
      throw new Error("REVIEW_NO_PANE: The agent's pane is closed.");
    });
    const onDone = vi.fn();
    await render(<FixSheet item={ITEM} api={{ fix }} onDone={onDone} onCancel={vi.fn()} />);
    await act(async () => {
      buttonWith('Put in chat')!.click();
    });
    expect(host.querySelector('.ck-rd-sheet-error')?.textContent).toBe("The agent's pane is closed.");
    expect(onDone).not.toHaveBeenCalled();
    expect(buttonWith('Put in chat')!.disabled).toBe(false);
  });

  it('Cancel closes without a call', async () => {
    const fix = vi.fn(async () => 'pane-1');
    const onCancel = vi.fn();
    await render(<FixSheet item={ITEM} api={{ fix }} onDone={vi.fn()} onCancel={onCancel} />);
    await act(async () => {
      buttonWith('Cancel')!.click();
    });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(fix).not.toHaveBeenCalled();
  });
});
