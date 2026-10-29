// @vitest-environment jsdom
// The desk as CockpitApp uses it: it shows the Keep and Ask to fix sheets itself (no onKeep or onFix given),
// opens on the first row when asked, and hands the detail the controls that do what the keys do.
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReviewItem } from '../../../lib/review-desk';
import { undo as undoThrow } from '../../../lib/review-desk-pending';
import { REVIEW_DESK_FULL } from '../../../dev/review-desk-fixtures';
import { ReviewDesk, type ReviewDetailControls } from './ReviewDesk';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const KEY_A = 'copy:mission-m1';
const KEY_ISSUE = 'issue:run-41';
const FIRST = REVIEW_DESK_FULL.items[0];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (Element.prototype as { scrollIntoView?: () => void }).scrollIntoView = () => {};
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  undoThrow();
  act(() => root.unmount());
  host.remove();
});

function fakeApi() {
  return {
    list: vi.fn(() => Promise.resolve(REVIEW_DESK_FULL)),
    keep: vi.fn(() => Promise.resolve({ sentence: 'Merged into main.', gone: true })),
    throwAway: vi.fn(() => Promise.resolve({ sentence: 'Done.', gone: true })),
    unmark: vi.fn(() => Promise.resolve()),
    fix: vi.fn(() => Promise.resolve('pane-codex-1')),
    detail: vi.fn((key: string) =>
      Promise.resolve({
        item: REVIEW_DESK_FULL.items.find((i) => i.key === key)!,
        patch: '',
        patchTruncated: false,
        extra: ['A file may hold a secret: .env'],
      }),
    ),
  };
}

async function mount(props: Record<string, unknown> = {}) {
  const api = fakeApi();
  const onOpenPane = vi.fn();
  await act(async () => {
    root.render(<ReviewDesk api={api} onOpenPane={onOpenPane} {...props} />);
  });
  await act(async () => {});
  return { api, onOpenPane };
}

const row = (key: string) => host.querySelector<HTMLElement>(`[data-rd-row="${key}"]`)!;
function press(key: string) {
  act(() => {
    (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
}
const buttonOf = (text: string) => [...document.querySelectorAll('button')].find((b) => b.textContent === text);

describe('Review Desk wired by the cockpit', () => {
  it('Y on a merge row opens the Merge sheet, and nothing is merged until Merge is pressed', async () => {
    const { api } = await mount();
    act(() => row(KEY_A).focus());
    press('y');
    expect(document.body.textContent).toContain('Merge into main?');
    expect(api.keep).not.toHaveBeenCalled();
    await act(async () => buttonOf('Merge')!.click());
    expect(api.keep).toHaveBeenCalledWith(KEY_A, FIRST.fingerprint);
    expect(document.body.textContent).not.toContain('Merge into main?');
    expect(host.textContent).toContain('Merged into main.');
  });

  it('Cancel closes the Merge sheet without calling Keep', async () => {
    const { api } = await mount();
    act(() => row(KEY_A).focus());
    press('y');
    act(() => buttonOf('Cancel')!.click());
    expect(document.body.textContent).not.toContain('Merge into main?');
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('Y on an issue row opens the push sheet with the push-check reasons from the detail', async () => {
    const { api } = await mount();
    act(() => row(KEY_ISSUE).focus());
    press('y');
    await act(async () => {});
    expect(document.body.textContent).toContain('Push as a draft pull request?');
    expect(api.detail).toHaveBeenCalledWith(KEY_ISSUE);
    expect(document.body.textContent).toContain('A file may hold a secret: .env');
    expect(document.body.textContent).toContain('Puppet Master opens a draft. It never merges.');
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('F opens the Ask to fix sheet; Put in chat fills the draft and opens that pane', async () => {
    const { api, onOpenPane } = await mount();
    act(() => row(KEY_A).focus());
    press('f');
    expect(document.body.textContent).toContain('Add a note (optional)');
    await act(async () => buttonOf('Put in chat')!.click());
    expect(api.fix).toHaveBeenCalledWith(KEY_A, FIRST.fingerprint, '');
    expect(onOpenPane).toHaveBeenCalledWith('pane-codex-1');
    expect(document.body.textContent).not.toContain('Add a note (optional)');
  });

  it('a caller that passes onKeep keeps its own sheet: the desk shows none', async () => {
    const onKeep = vi.fn();
    await mount({ onKeep });
    act(() => row(KEY_A).focus());
    press('y');
    expect(onKeep).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).not.toContain('Merge into main?');
  });

  it('openFirst opens the detail of the first row once the list is read', async () => {
    const seen: string[] = [];
    await mount({
      openFirst: true,
      renderDetail: (item: ReviewItem): ReactNode => {
        seen.push(item.key);
        return <p>Detail of {item.title}</p>;
      },
    });
    expect(host.querySelector('[data-rd-detail]')?.textContent).toBe(`Detail of ${FIRST.title}`);
    expect(seen[0]).toBe(KEY_A);
  });

  it('without openFirst no detail is open', async () => {
    await mount({ renderDetail: (item: ReviewItem) => <p>Detail of {item.title}</p> });
    expect(host.querySelector('[data-rd-detail]')).toBeNull();
  });

  it('the detail gets controls: keep opens the sheet, throwAway starts the Undo window', async () => {
    let controls: ReviewDetailControls | null = null;
    const { api } = await mount({
      openFirst: true,
      renderDetail: (_item: ReviewItem, ctl: ReviewDetailControls) => {
        controls = ctl;
        return <p>Detail</p>;
      },
    });
    expect(controls).not.toBeNull();
    act(() => controls!.keep(FIRST));
    expect(document.body.textContent).toContain('Merge into main?');
    act(() => buttonOf('Cancel')!.click());
    act(() => controls!.throwAway(FIRST));
    expect(host.querySelector('.ck-rd-toast')?.textContent).toContain('Throwing away');
    expect(api.throwAway).not.toHaveBeenCalled();
  });

  it('a Keep from the detail refreshes the list and shows the sentence', async () => {
    let controls: ReviewDetailControls | null = null;
    const { api } = await mount({
      openFirst: true,
      renderDetail: (_item: ReviewItem, ctl: ReviewDetailControls) => {
        controls = ctl;
        return <p>Detail</p>;
      },
    });
    const calls = api.list.mock.calls.length;
    await act(async () => controls!.kept({ sentence: 'Merged into main.', gone: true }, FIRST));
    expect(api.list.mock.calls.length).toBe(calls + 1);
    expect(host.textContent).toContain('Merged into main.');
  });
});
