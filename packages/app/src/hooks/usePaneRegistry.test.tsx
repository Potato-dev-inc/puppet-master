// @vitest-environment jsdom
// Overlapping pane-list refreshes must not let an older, slower answer overwrite a newer one: that
// is how a pane that had just closed stayed in the sidebar as a "ghost" row (live hunt, bug 4).
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '../lib/tauri';

type Resolver = (list: PaneInfo[]) => void;
const pending: Resolver[] = [];
const panesChanged: { cb: (() => void) | null } = { cb: null };

vi.mock('../lib/tauri', () => ({
  tauri: {
    listPanes: () => new Promise<PaneInfo[]>((resolve) => pending.push(resolve)),
    onPanesChanged: async (cb: () => void) => {
      panesChanged.cb = cb;
      return () => {};
    },
    onTerminalData: async () => () => {},
    onPtyResized: async () => () => {},
    onTerminalSnapshot: async () => () => {},
    onPtyStatus: async () => () => {},
    onPtyExit: async () => () => {},
    readScreenReplay: vi.fn(),
    readRawBuffer: vi.fn(),
    pendingResize: vi.fn(),
  },
}));

import { usePaneRegistry, type PaneRegistryApi } from './usePaneRegistry';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const pane = (id: string, created: number): PaneInfo => ({
  id, agent_type: 'claude', pid: 1, status: 'idle', created_at: created, last_output_at: null, cwd: 'C:\w', cols: 80, rows: 24,
});

describe('usePaneRegistry refresh ordering', () => {
  it('ignores an older pane list that arrives after a newer one', async () => {
    let latest: PaneRegistryApi | null = null;
    function Probe() {
      latest = usePaneRegistry();
      return null;
    }
    const root = createRoot(document.createElement('div'));
    await act(async () => root.render(<Probe />));
    // The mount refresh is in flight; a pane event starts a second one.
    expect(pending).toHaveLength(1);
    await act(async () => panesChanged.cb?.());
    expect(pending).toHaveLength(2);

    // The newer answer (the pane is gone) lands first, the stale one (pane still listed) after it.
    await act(async () => pending[1]([pane('a', 1)]));
    expect(latest!.paneList.map((p) => p.info.id)).toEqual(['a']);
    await act(async () => pending[0]([pane('a', 1), pane('ghost', 2)]));
    expect(latest!.paneList.map((p) => p.info.id)).toEqual(['a']);
    await act(async () => root.unmount());
  });
});
