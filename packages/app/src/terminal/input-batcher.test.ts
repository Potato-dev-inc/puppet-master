import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InputBatcher } from './input-batcher';

describe('InputBatcher', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('debounces input before flushing', () => {
    const flush = vi.fn();
    const batcher = new InputBatcher(flush, 8);

    batcher.push('a');
    batcher.push('b');
    expect(flush).not.toHaveBeenCalled();

    vi.advanceTimersByTime(8);
    expect(flush).toHaveBeenCalledWith('ab');
  });

  it('flushes pending input on dispose', () => {
    const flush = vi.fn();
    const batcher = new InputBatcher(flush, 8);
    batcher.push('x');
    batcher.dispose();
    expect(flush).toHaveBeenCalledWith('x');
  });
});
