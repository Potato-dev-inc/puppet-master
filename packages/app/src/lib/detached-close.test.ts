import { describe, expect, it } from 'vitest';
import { detachedCloseAction } from './detached-close';

describe('detachedCloseAction', () => {
  it('closes a pane window (the backend kills its pane; nothing reattaches)', () => {
    expect(detachedCloseAction({ workerHost: false })).toBe('close');
  });
  it('worker hosts kill the pane and quit', () => {
    expect(detachedCloseAction({ workerHost: true })).toBe('kill-and-exit');
  });
});
