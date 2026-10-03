import { describe, expect, it } from 'vitest';
import { detachedPaneUrl } from './detached-pane-window';

describe('detachedPaneUrl', () => {
  it('loads index.html so production webviews can pop out', () => {
    expect(detachedPaneUrl('pane-1')).toBe(
      'index.html?terminal=1&pane=pane-1&detached=1',
    );
    expect(detachedPaneUrl('pane-1', { workerHost: true })).toBe(
      'index.html?terminal=1&pane=pane-1&detached=1&worker-host=1',
    );
  });
});
