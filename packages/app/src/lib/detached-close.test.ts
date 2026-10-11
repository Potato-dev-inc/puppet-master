import { describe, expect, it } from 'vitest';
import { detachedCloseAction } from './detached-close';

describe('detachedCloseAction', () => {
  it('reattaches a live pane', () => {
    expect(detachedCloseAction({ workerHost: false, paneExists: true })).toBe('reattach');
  });
  it('just closes when the pane is gone', () => {
    expect(detachedCloseAction({ workerHost: false, paneExists: false })).toBe('close');
  });
  it('worker hosts kill the pane and quit', () => {
    expect(detachedCloseAction({ workerHost: true, paneExists: false })).toBe('kill-and-exit');
  });
});
