import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  REVIEW_DESK_EVENT,
  REVIEW_REFRESH_DEBOUNCE_MS,
  REVIEW_REFRESH_EVENTS,
  type ReviewDesk,
  type ReviewItem,
  levelGlyph,
  onReviewDeskRefresh,
  reviewDeskApi,
  reviewDeskCount,
  reviewDeskDetail,
  reviewDeskError,
  reviewDeskErrorText,
  reviewDeskFix,
  reviewDeskKeep,
  reviewDeskList,
  reviewDeskThrowAway,
  reviewDeskUnmark,
} from './review-desk';

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

beforeEach(() => setTauri(true));

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  vi.useRealTimers();
  setTauri(false);
});

/** One desk row as `review_desk/view.rs` sends it (camelCase, Option as null). Fixture text is placeholder only. */
const item: ReviewItem = {
  key: 'copy:cart-copy',
  source: 'copy',
  fingerprint: 'a1b2c3d4e5f6',
  title: 'Make the cart total round to cents',
  agentLabel: 'Codex',
  whereLabel: 'Separate copy of main',
  paneId: 'pane-1',
  finishedAtMs: 1_800_000_000_000,
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
  reasons: [{ flag: 'tests_weakened', sentence: 'The tests were weakened: 2 tests were skipped in cart.test.ts' }],
  keep: { kind: 'merge', label: 'Merge...', into: 'main', why: null },
  throw: { kind: 'discard', why: null },
  fixPaneId: 'pane-1',
  fixWhyNot: null,
};

const desk: ReviewDesk = {
  items: [item],
  counts: { total: 1, high: 1, check: 0, low: 0 },
  notes: ['Could not read Night Shift. Its tasks are not listed.'],
};

describe('review desk commands', () => {
  it('sends each wrapper to its command with the argument keys of section 3.6', async () => {
    invoke.mockResolvedValue(undefined);
    await reviewDeskList();
    await reviewDeskCount();
    await reviewDeskDetail('copy:cart-copy');
    await reviewDeskKeep('copy:cart-copy', 'a1b2c3d4e5f6');
    await reviewDeskThrowAway('copy:cart-copy', 'a1b2c3d4e5f6');
    await reviewDeskUnmark('pane-1');
    await reviewDeskFix('copy:cart-copy', 'a1b2c3d4e5f6', 'Check the rounding');

    expect(invoke.mock.calls).toEqual([
      ['review_desk_list', undefined],
      ['review_desk_count', undefined],
      ['review_desk_detail', { key: 'copy:cart-copy' }],
      ['review_desk_keep', { key: 'copy:cart-copy', fingerprint: 'a1b2c3d4e5f6' }],
      ['review_desk_throw_away', { key: 'copy:cart-copy', fingerprint: 'a1b2c3d4e5f6' }],
      ['review_desk_unmark', { paneId: 'pane-1' }],
      ['review_desk_fix', { key: 'copy:cart-copy', fingerprint: 'a1b2c3d4e5f6', note: 'Check the rounding' }],
    ]);
  });

  it('returns the desk list, the count and the fix pane as the command sent them', async () => {
    invoke.mockResolvedValueOnce(desk).mockResolvedValueOnce(4).mockResolvedValueOnce('pane-9');

    await expect(reviewDeskList()).resolves.toEqual(desk);
    await expect(reviewDeskCount()).resolves.toBe(4);
    await expect(reviewDeskFix('copy:cart-copy', 'a1b2c3d4e5f6', '')).resolves.toBe('pane-9');
  });

  it('exposes the same calls as one object for injected components', async () => {
    invoke.mockResolvedValue(desk);
    await expect(reviewDeskApi.list()).resolves.toEqual(desk);
    await reviewDeskApi.unmark('pane-2');
    expect(invoke).toHaveBeenLastCalledWith('review_desk_unmark', { paneId: 'pane-2' });
    expect(reviewDeskApi.keep).toBe(reviewDeskKeep);
    expect(reviewDeskApi.throwAway).toBe(reviewDeskThrowAway);
  });

  it('refuses to call the backend outside the desktop shell', async () => {
    setTauri(false);
    await expect(reviewDeskList()).rejects.toThrow('Desktop shell required: review_desk_list');
    await expect(reviewDeskKeep('copy:x', 'f')).rejects.toThrow('Desktop shell required: review_desk_keep');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('levelGlyph', () => {
  it('maps each level to its glyph', () => {
    expect(levelGlyph('high')).toBe('●');
    expect(levelGlyph('check')).toBe('◐');
    expect(levelGlyph('low')).toBe('○');
  });
});

describe('reviewDeskError', () => {
  it('reads an OperationError object', () => {
    const err = { code: 'REVIEW_ITEM_CHANGED', message: 'It changed. Look again.', recoverable: true };
    expect(reviewDeskError(err)).toEqual({ code: 'REVIEW_ITEM_CHANGED', message: 'It changed. Look again.' });
  });

  it('reads a CODE: message string', () => {
    expect(reviewDeskError('REVIEW_BUSY: This is already running.')).toEqual({
      code: 'REVIEW_BUSY',
      message: 'This is already running.',
    });
  });

  it('keeps a plain sentence without a code', () => {
    expect(reviewDeskError(new Error('Git is not installed.'))).toEqual({ code: '', message: 'Git is not installed.' });
    expect(reviewDeskErrorText('MERGE_CONFLICT: Two copies changed the same line.')).toBe(
      'Two copies changed the same line.',
    );
  });

  it('falls back to a plain sentence for anything else', () => {
    expect(reviewDeskError(undefined)).toEqual({ code: '', message: 'Something went wrong. Try again.' });
    expect(reviewDeskError({ code: 'REVIEW_BUSY', message: '' })).toEqual({
      code: 'REVIEW_BUSY',
      message: 'Something went wrong. Try again.',
    });
  });
});

describe('onReviewDeskRefresh', () => {
  it('listens to the six refresh events of section 3.5 H', () => {
    listen.mockResolvedValue(vi.fn());
    onReviewDeskRefresh(() => {});
    expect(listen.mock.calls.map((call) => call[0])).toEqual([...REVIEW_REFRESH_EVENTS]);
    expect(REVIEW_DESK_EVENT).toBe('review-desk://changed');
    expect(REVIEW_REFRESH_EVENTS).toHaveLength(6);
  });

  it('calls once, after the debounce, for a burst of events', async () => {
    vi.useFakeTimers();
    const handlers: Array<() => void> = [];
    listen.mockImplementation(async (_name: string, handler: () => void) => {
      handlers.push(handler);
      return vi.fn();
    });
    const cb = vi.fn();
    onReviewDeskRefresh(cb);
    await vi.runOnlyPendingTimersAsync();

    handlers[0]();
    vi.advanceTimersByTime(300);
    handlers[3]();
    vi.advanceTimersByTime(300);
    handlers[5]();
    expect(cb).not.toHaveBeenCalled();

    vi.advanceTimersByTime(REVIEW_REFRESH_DEBOUNCE_MS - 1);
    expect(cb).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it('removes every listener and any pending call on unsubscribe', async () => {
    vi.useFakeTimers();
    const offs: Array<ReturnType<typeof vi.fn>> = [];
    const handlers: Array<() => void> = [];
    listen.mockImplementation(async (_name: string, handler: () => void) => {
      const off = vi.fn();
      offs.push(off);
      handlers.push(handler);
      return off;
    });
    const cb = vi.fn();
    const stop = onReviewDeskRefresh(cb);
    await vi.runOnlyPendingTimersAsync();

    handlers[1]();
    stop();
    vi.advanceTimersByTime(REVIEW_REFRESH_DEBOUNCE_MS * 2);

    expect(cb).not.toHaveBeenCalled();
    expect(offs).toHaveLength(6);
    for (const off of offs) expect(off).toHaveBeenCalledTimes(1);
  });

  it('removes a listener that finishes registering after unsubscribe', async () => {
    const off = vi.fn();
    const late: { finish?: (value: () => void) => void } = {};
    listen.mockImplementationOnce(
      () =>
        new Promise<() => void>((resolve) => {
          late.finish = resolve;
        }),
    );
    listen.mockResolvedValue(vi.fn());
    const stop = onReviewDeskRefresh(() => {});
    stop();
    late.finish?.(off);
    await Promise.resolve();
    await Promise.resolve();
    expect(off).toHaveBeenCalledTimes(1);
  });

  it('does nothing outside the desktop shell', () => {
    setTauri(false);
    const stop = onReviewDeskRefresh(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});
