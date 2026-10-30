// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PANE_ACTION_TTL_MS, requestPaneAction, subscribePaneActions, takePendingPaneAction } from './pane-actions';

afterEach(() => {
  takePendingPaneAction('a');
  takePendingPaneAction('b');
});

describe('pane actions', () => {
  it('runs a request on the view of that pane only', () => {
    const a = vi.fn();
    const b = vi.fn();
    const offA = subscribePaneActions('a', a);
    const offB = subscribePaneActions('b', b);
    requestPaneAction('b', 'reflow');
    expect(b).toHaveBeenCalledWith('reflow');
    expect(a).not.toHaveBeenCalled();
    offA();
    offB();
  });

  it('a request made before the view exists waits for it, once', () => {
    requestPaneAction('a', 'rename');
    const run = vi.fn();
    const off = subscribePaneActions('a', run);
    expect(run).toHaveBeenCalledWith('rename');
    off();
    const again = vi.fn();
    subscribePaneActions('a', again)();
    expect(again).not.toHaveBeenCalled();
  });

  it('a request already run by a mounted view does not run again on the next mount', () => {
    const live = vi.fn();
    const off = subscribePaneActions('a', live);
    requestPaneAction('a', 'details');
    off();
    const later = vi.fn();
    subscribePaneActions('a', later)();
    expect(live).toHaveBeenCalledTimes(1);
    expect(later).not.toHaveBeenCalled();
  });

  it('drops a request nobody picked up in time', () => {
    requestPaneAction('a', 'close', 1000);
    expect(takePendingPaneAction('a', 1000 + PANE_ACTION_TTL_MS + 1)).toBeNull();
    requestPaneAction('a', 'close', 1000);
    expect(takePendingPaneAction('a', 1000 + PANE_ACTION_TTL_MS)).toBe('close');
  });

  it('does not hand a request to another pane', () => {
    requestPaneAction('a', 'copy');
    expect(takePendingPaneAction('b')).toBeNull();
    expect(takePendingPaneAction('a')).toBe('copy');
  });
});
