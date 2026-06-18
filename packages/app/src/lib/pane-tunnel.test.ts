import { describe, expect, it, vi } from 'vitest';
import {
  bindPaneTunnelSubscribe,
  createPaneTunnelState,
  ingestPaneTunnelData,
  makePaneTunnelTransport,
  mergePaneDimensions,
  setPaneTunnelPane,
  subscribePaneTunnelData,
} from './pane-tunnel';
import type { PaneInfo } from '@puppet-master/shared';

describe('pane-tunnel', () => {
  it('makePaneTunnelTransport writes through the bridge and ignores resize', async () => {
    const writeInput = vi.fn().mockResolvedValue(undefined);
    const bridge = { writeInput } as unknown as Parameters<typeof makePaneTunnelTransport>[0];
    const transport = makePaneTunnelTransport(bridge, 'pane-1');

    await transport.resize(120, 40);
    await transport.writeInput('ls', true);

    expect(writeInput).toHaveBeenCalledWith('pane-1', 'ls', true);
  });

  it('ingestPaneTunnelData only accepts the bound pane id', async () => {
    const state = createPaneTunnelState('mobile');
    setPaneTunnelPane(state, 'orch');

    const received: Uint8Array[] = [];
    const bridge = {
      readRawBuffer: vi.fn().mockResolvedValue([]),
    } as unknown as Parameters<typeof subscribePaneTunnelData>[1];

    subscribePaneTunnelData(state, bridge, (chunk) => received.push(chunk));

    ingestPaneTunnelData(state, 'other', Uint8Array.from([1]));
    ingestPaneTunnelData(state, 'orch', Uint8Array.from([2]));

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(received).toEqual([Uint8Array.from([2])]);
  });

  it('bindPaneTunnelSubscribe ignores other pane ids', () => {
    const subscribe = vi.fn(() => () => {});
    const bound = bindPaneTunnelSubscribe(subscribe, 'pane-a');

    bound('pane-b', () => {});
    expect(subscribe).not.toHaveBeenCalled();

    bound('pane-a', () => {});
    expect(subscribe).toHaveBeenCalledTimes(1);
  });

  it('mergePaneDimensions overlays cols and rows', () => {
    const info = {
      id: 'pane-a',
      cols: 80,
      rows: 24,
    } as PaneInfo;

    expect(mergePaneDimensions(info, 100, 40)).toEqual({ ...info, cols: 100, rows: 40 });
    expect(mergePaneDimensions(info, 80, 24)).toBe(info);
  });
});
