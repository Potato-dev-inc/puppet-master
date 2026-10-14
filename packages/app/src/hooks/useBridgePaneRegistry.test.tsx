// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import type { PaneInfo } from '@puppet-master/shared';
import type { BridgeClient } from '../lib/bridge';
import { useBridgePaneRegistry, type BridgePaneRegistryApi } from './useBridgePaneRegistry';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const pane: PaneInfo = {
  id: 'p1', agent_type: 'claude', pid: 1, status: 'idle', created_at: 1, last_output_at: null, cwd: 'C:\\w', cols: 121, rows: 36,
};

async function mountRegistry(
  extra: Partial<BridgeClient> = {},
  onInputRefused?: (paneId: string, error: Error) => void,
): Promise<{ api: () => BridgePaneRegistryApi; unmount: () => Promise<void> }> {
  const bridge = {
    listPanes: vi.fn().mockResolvedValue([pane]),
    readRawBuffer: vi.fn().mockResolvedValue([]),
    ...extra,
  } as unknown as BridgeClient;
  let latest: BridgePaneRegistryApi | null = null;
  function Probe() {
    latest = useBridgePaneRegistry(bridge, undefined, undefined, onInputRefused);
    return null;
  }
  const root = createRoot(document.createElement('div'));
  await act(async () => root.render(<Probe />));
  return { api: () => latest!, unmount: async () => act(async () => root.unmount()) };
}

describe('useBridgePaneRegistry', () => {
  it('keeps a live listener across a resize when the viewer resizes in place', async () => {
    const { api, unmount } = await mountRegistry();
    const received: string[] = [];
    await act(async () => {
      api().subscribePaneData('p1', (data) => received.push(new TextDecoder().decode(data)));
    });
    await act(async () => api().ingestTerminalData('p1', new TextEncoder().encode('before')));
    await act(async () => api().updatePaneDimensions('p1', 50, 26, true));
    await act(async () => api().ingestTerminalData('p1', new TextEncoder().encode('after')));
    expect(received.join('')).toContain('after');
    expect(api().panes.get('p1')?.info.cols).toBe(50);
    await unmount();
  });

  it('resets the stream for remounting mirrors (default)', async () => {
    const { api, unmount } = await mountRegistry();
    const received: string[] = [];
    await act(async () => {
      api().subscribePaneData('p1', (data) => received.push(new TextDecoder().decode(data)));
    });
    await act(async () => api().updatePaneDimensions('p1', 50, 26));
    await act(async () => api().ingestTerminalData('p1', new TextEncoder().encode('after')));
    expect(received.join('')).not.toContain('after');
    await unmount();
  });

  // VERIFY-R11: typing into an agent-run pane from a loopback tab was refused with a 403 and nothing
  // on screen said so. The transport now hands the refusal to the app.
  it('hands a refused write (403) to the app instead of dropping it', async () => {
    const refusal = new Error('bridge POST /panes/p1/input -> 403: {"code":"AUTHORIZATION_DENIED"}');
    const writeInput = vi.fn().mockRejectedValue(refusal);
    const seen: Array<[string, Error]> = [];
    const { api, unmount } = await mountRegistry({ writeInput }, (paneId, error) => seen.push([paneId, error]));
    await act(async () => {
      await api().makeTransport('p1').writeInput('x', false);
    });
    expect(seen).toEqual([['p1', refusal]]);
    await unmount();
  });
});
