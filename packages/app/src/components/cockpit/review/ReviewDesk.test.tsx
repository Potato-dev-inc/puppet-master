// @vitest-environment jsdom
// The Review Desk list: rows in the order given, the keys move and act like the buttons would, Throw away waits
// its 10 s Undo window, "Looks good" runs at once, and no key fires inside an input or a sheet.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReviewDesk as ReviewList, ReviewItem } from '../../../lib/review-desk';
import { undo as undoThrow } from '../../../lib/review-desk-pending';
import { REVIEW_DESK_EMPTY, REVIEW_DESK_FULL } from '../../../dev/review-desk-fixtures';
import { ReviewDesk, type ReviewDeskProps } from './ReviewDesk';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const KEYS = REVIEW_DESK_FULL.items.map((i) => i.key);
const TITLE_A = 'Make the cart total round to cents';
const KEY_A = 'copy:mission-m1';
const KEY_C = 'folder:pane-claude-1:4-6';

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
  vi.useRealTimers();
});

/** A fake of the backend calls. Each one resolves at once. */
function fakeApi(desk: ReviewList = REVIEW_DESK_FULL) {
  return {
    list: vi.fn(() => Promise.resolve(desk)),
    keep: vi.fn(() => Promise.resolve({ sentence: 'Done.', gone: true })),
    throwAway: vi.fn(() => Promise.resolve({ sentence: 'Done.', gone: true })),
    unmark: vi.fn(() => Promise.resolve()),
    fix: vi.fn(() => Promise.resolve('pane-codex-1')),
  };
}

async function mount(extra: Partial<ReviewDeskProps> = {}, desk?: ReviewList) {
  const api = fakeApi(desk);
  const props = { api, onOpenPane: vi.fn(), onClose: vi.fn(), onKeep: vi.fn(), onFix: vi.fn(), ...extra };
  await act(async () => {
    root.render(<ReviewDesk {...props} />);
  });
  await act(async () => {});
  return { api, props };
}

/** One key press, sent to the focused element (or the body) so it bubbles to the window like a real key. */
function press(key: string, target: EventTarget | null = document.activeElement ?? document.body) {
  act(() => {
    (target ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  });
}

const rowKeys = (): (string | undefined)[] =>
  Array.from(host.querySelectorAll<HTMLElement>('[data-rd="item"]')).map((el) => el.dataset.rdRow);
const row = (key: string): HTMLElement | null => host.querySelector<HTMLElement>(`[data-rd-row="${key}"]`);
const toast = (): HTMLElement | null => host.querySelector<HTMLElement>('.ck-rd-toast');

describe('Review Desk list', () => {
  it('lists the rows in the order given, with the group header and the counts', async () => {
    await mount();
    expect(rowKeys()).toEqual(KEYS);
    expect(host.querySelector('.ck-rd-heading')?.textContent).toBe('Review · 5');
    expect(host.querySelector('.ck-rd-counts')?.textContent).toBe('High risk 2 · Check first 1 · Low risk 2');
    expect(host.querySelector('[data-rd="group"]')?.textContent).toBe('Builder and helpers');
  });

  it('shows each row with its title, its first reason, or "No flags"', async () => {
    await mount();
    expect(row(KEY_A)?.textContent).toContain(TITLE_A);
    expect(row(KEY_A)?.textContent).toContain('The tests were weakened: 2 tests were skipped in cart.test.ts');
    expect(row('folder:pane-codex-2:1-1')?.textContent).toContain('No flags');
    expect(row(KEY_A)?.querySelector('.ck-rd-files')?.textContent).toBe('4 files +52 −20');
  });

  it('shows the empty line and a plain note when nothing is waiting', async () => {
    await mount({}, REVIEW_DESK_EMPTY);
    expect(host.textContent).toContain('Nothing to review. Finished work from your agents shows up here.');
    expect(host.textContent).toContain('Could not read Night Shift. Its tasks are not listed.');
  });

  it('shows the plain sentence when the list cannot be read', async () => {
    const api = fakeApi();
    api.list = vi.fn(() => Promise.reject(new Error('Something broke')));
    await act(async () => {
      root.render(<ReviewDesk api={api} />);
    });
    await act(async () => {});
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Something broke');
  });
});

describe('Review Desk keys', () => {
  it('J and K move focus between the rows, and never stop on the group header', async () => {
    await mount();
    press('j', document.body);
    expect(document.activeElement).toBe(row(KEYS[0]));
    press('j');
    expect(document.activeElement).toBe(row(KEYS[1]));
    press('j');
    expect(document.activeElement).toBe(row(KEYS[2]));
    press('k');
    expect(document.activeElement).toBe(row(KEYS[1]));
  });

  it('Enter opens the detail; Esc closes it, then closes the desk', async () => {
    const { props } = await mount({ renderDetail: (item: ReviewItem) => <p>Detail of {item.title}</p> });
    press('j', document.body);
    press('Enter');
    expect(host.querySelector('[data-rd-detail]')?.textContent).toBe(`Detail of ${TITLE_A}`);
    press('Escape');
    expect(host.querySelector('[data-rd-detail]')).toBeNull();
    expect(props.onClose).not.toHaveBeenCalled();
    press('Escape');
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it('X starts a 10 s Undo window: the row hides, the line says so, and nothing is thrown away yet', async () => {
    vi.useFakeTimers();
    const { api } = await mount();
    press('j', document.body);
    press('x');
    expect(row(KEY_A)).toBeNull();
    expect(toast()?.textContent).toBe(`Throwing away "${TITLE_A}" in 10 s · Undo (Z)`);
    expect(api.throwAway).not.toHaveBeenCalled();
  });

  it('when the Undo window ends, the throw runs once with the key and fingerprint the person saw', async () => {
    vi.useFakeTimers();
    const { api } = await mount();
    press('j', document.body);
    press('x');
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(api.throwAway).toHaveBeenCalledTimes(1);
    expect(api.throwAway).toHaveBeenCalledWith(KEY_A, 'fp-a1');
  });

  it('Z undoes a pending throw: the row comes back and nothing is thrown away', async () => {
    vi.useFakeTimers();
    const { api } = await mount();
    press('j', document.body);
    press('x');
    press('z', document.body);
    expect(row(KEY_A)).not.toBeNull();
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(api.throwAway).not.toHaveBeenCalled();
  });

  it('a second X while one throw waits is refused with a plain sentence', async () => {
    vi.useFakeTimers();
    await mount();
    press('j', document.body);
    press('x');
    press('j', document.body);
    press('x');
    expect(host.querySelector('.ck-rd-notice')?.textContent).toBe('Finish the one in progress first');
    expect(toast()?.textContent).toContain(TITLE_A);
  });

  it('Y on a "Looks good" row calls keep at once and shows the Undo line; Z unmarks it', async () => {
    const { api, props } = await mount();
    press('j', document.body);
    press('j');
    press('j');
    expect(document.activeElement).toBe(row(KEY_C));
    press('y');
    await act(async () => {});
    expect(api.keep).toHaveBeenCalledWith(KEY_C, 'fp-c1');
    expect(props.onKeep).not.toHaveBeenCalled();
    expect(toast()?.textContent).toBe('Marked as looked at · Undo (Z)');
    press('z', document.body);
    await act(async () => {});
    expect(api.unmark).toHaveBeenCalledWith('pane-kilo-1');
  });

  it('Y on a merge row hands the item to onKeep and does not call the api', async () => {
    const { api, props } = await mount();
    press('j', document.body);
    press('y');
    expect(props.onKeep).toHaveBeenCalledTimes(1);
    expect(props.onKeep).toHaveBeenCalledWith(expect.objectContaining({ key: KEY_A }));
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('F hands the item to onFix, and P opens the pane of the row', async () => {
    const { props } = await mount();
    press('j', document.body);
    press('f');
    expect(props.onFix).toHaveBeenCalledWith(expect.objectContaining({ key: KEY_A }));
    press('p');
    expect(props.onOpenPane).toHaveBeenCalledWith('pane-codex-1');
  });

  it('keys are ignored inside an input and inside a sheet', async () => {
    const { api, props } = await mount({
      renderDetail: () => (
        <>
          <input aria-label="Note" />
          <div className="ck-sheet" aria-modal="true">
            <button type="button">Keep</button>
          </div>
        </>
      ),
    });
    press('j', document.body);
    press('Enter');
    const input = host.querySelector<HTMLInputElement>('input');
    expect(input).not.toBeNull();
    input?.focus();
    press('j', input);
    expect(document.activeElement).toBe(input);
    const sheetButton = host.querySelector<HTMLButtonElement>('.ck-sheet button');
    press('y', sheetButton);
    expect(props.onKeep).not.toHaveBeenCalled();
    expect(api.keep).not.toHaveBeenCalled();
  });

  it('a throw the backend refuses shows its sentence, and the row comes back', async () => {
    vi.useFakeTimers();
    const { api } = await mount();
    api.throwAway = vi.fn(() => Promise.reject(new Error('REVIEW_CANNOT_THROW: The agent is still working.')));
    press('j', document.body);
    press('x');
    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });
    expect(host.querySelector('.ck-rd-notice')?.textContent).toBe('The agent is still working.');
    expect(row(KEY_A)).not.toBeNull();
  });
});
