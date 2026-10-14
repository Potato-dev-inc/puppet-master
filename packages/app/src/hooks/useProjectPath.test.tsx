// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { subscribeSettingsRefresh } from '../lib/settings-sync';
import { tauri } from '../lib/tauri';
import { useProjectPath } from './useProjectPath';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const state = vi.hoisted(() => ({
  savedPath: null as string | null,
  registryPath: '' as string,
  refresh: null as null | ((info: { reason: 'changed' | 'show' | 'focus' }) => void),
  unsubscribe: vi.fn(),
}));

vi.mock('../lib/settings', () => ({
  loadSettings: vi.fn(async () => ({ project_path: state.savedPath })),
  patchSettings: vi.fn(async () => ({})),
}));

vi.mock('../lib/tauri', () => ({
  tauri: {
    getProjectPath: vi.fn(async () => state.registryPath),
    setProjectPath: vi.fn(async () => undefined),
  },
}));

vi.mock('../lib/settings-sync', () => ({
  subscribeSettingsRefresh: vi.fn((cb: (info: { reason: 'changed' | 'show' | 'focus' }) => void) => {
    state.refresh = cb;
    return state.unsubscribe;
  }),
}));

type Hook = ReturnType<typeof useProjectPath>;

function Probe(props: { seen: { current: Hook | null } }) {
  props.seen.current = useProjectPath();
  return null;
}

async function flush() {
  await act(async () => {
    for (let i = 0; i < 10; i += 1) await Promise.resolve();
  });
}

describe('useProjectPath', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.savedPath = 'D:/one';
    state.registryPath = 'D:/one';
    state.refresh = null;
  });

  it('follows a project_path written behind its back', async () => {
    const root = createRoot(document.createElement('div'));
    const seen: { current: Hook | null } = { current: null };
    await act(async () => {
      root.render(<Probe seen={seen} />);
    });
    await flush();
    expect(seen.current?.projectPath).toBe('D:/one');

    state.savedPath = 'D:/two';
    expect(state.refresh).not.toBeNull();
    await act(async () => {
      state.refresh?.({ reason: 'changed' });
    });
    await flush();
    expect(seen.current?.projectPath).toBe('D:/two');
    // A refresh follows the saved value; it must not write it back.
    expect(tauri.setProjectPath).not.toHaveBeenCalledWith('D:/two');

    await act(async () => root.unmount());
  });

  it('ignores an empty or root path on refresh', async () => {
    const root = createRoot(document.createElement('div'));
    const seen: { current: Hook | null } = { current: null };
    await act(async () => {
      root.render(<Probe seen={seen} />);
    });
    await flush();
    expect(seen.current?.projectPath).toBe('D:/one');

    state.savedPath = null;
    await act(async () => {
      state.refresh?.({ reason: 'changed' });
    });
    await flush();
    expect(seen.current?.projectPath).toBe('D:/one');

    state.savedPath = '/';
    await act(async () => {
      state.refresh?.({ reason: 'changed' });
    });
    await flush();
    expect(seen.current?.projectPath).toBe('D:/one');

    await act(async () => root.unmount());
  });

  it('unsubscribes on unmount', async () => {
    const root = createRoot(document.createElement('div'));
    const seen: { current: Hook | null } = { current: null };
    await act(async () => {
      root.render(<Probe seen={seen} />);
    });
    await flush();
    expect(subscribeSettingsRefresh).toHaveBeenCalledTimes(1);
    expect(state.unsubscribe).not.toHaveBeenCalled();

    await act(async () => root.unmount());
    expect(state.unsubscribe).toHaveBeenCalledTimes(1);
  });
});
