import { describe, expect, it } from 'vitest';
import { detachedPaneUrl, runOncePerKey } from './detached-pane-window';

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

describe('runOncePerKey', () => {
  it('shares one running task between concurrent callers of the same key', async () => {
    let calls = 0;
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const task = async () => {
      calls += 1;
      await gate;
    };
    const a = runOncePerKey('pane-a', task);
    const b = runOncePerKey('pane-a', task);
    const c = runOncePerKey('pane-b', task);
    expect(a).toBe(b);
    expect(calls).toBe(2);
    release();
    await Promise.all([a, b, c]);
    await runOncePerKey('pane-a', task);
    expect(calls).toBe(3);
  });
});
