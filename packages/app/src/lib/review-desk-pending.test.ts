import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  FINISH_FIRST_SENTENCE,
  UNDO_MS,
  pending,
  startThrow,
  subscribe,
  undo,
} from './review-desk-pending';

const item = { key: 'copy:cart-copy', title: 'Rename helper', fingerprint: 'a1b2c3d4e5f6' };
const other = { key: 'copy:money-copy', title: 'Round money', fingerprint: '0f0f0f0f0f0f' };

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-29T08:00:00Z'));
});

afterEach(() => {
  // Leave the module store empty for the next test.
  undo();
  vi.useRealTimers();
});

describe('review desk throw-away window', () => {
  it('uses a 10 second window', () => {
    expect(UNDO_MS).toBe(10_000);
  });

  it('shows the pending row and calls the throw once the window ends', async () => {
    const run = vi.fn().mockResolvedValue(undefined);
    expect(startThrow(item, run)).toEqual({ started: true });

    expect(pending()).toEqual({
      key: item.key,
      title: item.title,
      fingerprint: item.fingerprint,
      endsAt: Date.parse('2026-10-29T08:00:00Z') + UNDO_MS,
    });
    vi.advanceTimersByTime(UNDO_MS - 1);
    expect(run).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(run).toHaveBeenCalledTimes(1);
    await vi.runOnlyPendingTimersAsync();
    expect(pending()).toBeNull();
  });

  it('never calls the throw when Undo comes first', () => {
    const run = vi.fn().mockResolvedValue(undefined);
    startThrow(item, run);

    expect(undo()).toBe(true);
    expect(pending()).toBeNull();
    vi.advanceTimersByTime(UNDO_MS * 2);
    expect(run).not.toHaveBeenCalled();
  });

  it('does nothing on Undo when nothing is pending', () => {
    expect(undo()).toBe(false);
    expect(pending()).toBeNull();
  });

  it('refuses a second throw while one is pending and commits nothing', () => {
    const first = vi.fn().mockResolvedValue(undefined);
    const second = vi.fn().mockResolvedValue(undefined);
    startThrow(item, first);

    expect(startThrow(other, second)).toEqual({ started: false, sentence: FINISH_FIRST_SENTENCE });
    expect(pending()?.key).toBe(item.key);

    vi.advanceTimersByTime(UNDO_MS);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
  });

  it('allows a new throw once the first one is undone', () => {
    const second = vi.fn().mockResolvedValue(undefined);
    startThrow(item, vi.fn().mockResolvedValue(undefined));
    undo();

    expect(startThrow(other, second)).toEqual({ started: true });
    vi.advanceTimersByTime(UNDO_MS);
    expect(second).toHaveBeenCalledTimes(1);
  });

  it('passes a failed throw to onError as a plain sentence', async () => {
    const onError = vi.fn();
    const run = vi.fn().mockRejectedValue({ code: 'REVIEW_ITEM_CHANGED', message: 'It changed. Look again.' });
    startThrow(item, run, onError);

    vi.advanceTimersByTime(UNDO_MS);
    await vi.runOnlyPendingTimersAsync();
    expect(onError).toHaveBeenCalledWith('It changed. Look again.');
    expect(pending()).toBeNull();
  });

  it('reads a CODE: message string the same way', async () => {
    const onError = vi.fn();
    startThrow(item, () => Promise.reject(new Error('REVIEW_BUSY: This is already running.')), onError);

    vi.advanceTimersByTime(UNDO_MS);
    await vi.runOnlyPendingTimersAsync();
    expect(onError).toHaveBeenCalledWith('This is already running.');
  });

  it('does not call onError when the throw succeeds', async () => {
    const onError = vi.fn();
    startThrow(item, vi.fn().mockResolvedValue(undefined), onError);

    vi.advanceTimersByTime(UNDO_MS);
    await vi.runOnlyPendingTimersAsync();
    expect(onError).not.toHaveBeenCalled();
  });

  it('tells subscribers about every change and stops after unsubscribe', () => {
    const seen: Array<string | null> = [];
    const stop = subscribe((p) => seen.push(p ? p.key : null));

    startThrow(item, vi.fn().mockResolvedValue(undefined));
    undo();
    stop();
    startThrow(other, vi.fn().mockResolvedValue(undefined));

    expect(seen).toEqual([item.key, null]);
  });

  it('keeps the same snapshot object until the state changes', () => {
    startThrow(item, vi.fn().mockResolvedValue(undefined));
    const first = pending();
    expect(pending()).toBe(first);
    undo();
    expect(pending()).toBeNull();
  });
});
