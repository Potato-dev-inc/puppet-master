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

  // VERIFY-R11: the bridge's 403 for a loopback PWA tab typing into an agent-run pane was swallowed.
  it('reports a 403 from the managed-pane guard instead of failing silently, and still throws other errors', async () => {
    const refusal = new Error(
      'bridge POST /panes/pane-1/input -> 403: {"code":"AUTHORIZATION_DENIED","error":"managed pane mutations require an MCP session id"}',
    );
    const writeInput = vi.fn().mockRejectedValueOnce(refusal).mockRejectedValueOnce(new Error('Failed to fetch'));
    const bridge = { writeInput } as unknown as Parameters<typeof makePaneTunnelTransport>[0];
    const onRefused = vi.fn();
    const transport = makePaneTunnelTransport(bridge, 'pane-1', undefined, onRefused);

    await expect(transport.writeInput('x', false)).resolves.toBeUndefined();
    expect(onRefused).toHaveBeenCalledTimes(1);
    expect(onRefused).toHaveBeenCalledWith('pane-1', refusal);
    await expect(transport.writeInput('y', false)).rejects.toThrow('Failed to fetch');
    expect(onRefused).toHaveBeenCalledTimes(1);
  });

  it('ingestPaneTunnelData only accepts the bound pane id', async () => {
    const state = createPaneTunnelState('mobile');
    setPaneTunnelPane(state, 'orch');

    const received: Uint8Array[] = [];
    const bridge = {
      readRawBuffer: vi.fn().mockResolvedValue([]),
    } as unknown as Parameters<typeof subscribePaneTunnelData>[1];

    subscribePaneTunnelData(state, bridge, 'orch', (chunk) => received.push(chunk));
    await vi.waitFor(() => {
      expect(bridge.readRawBuffer).toHaveBeenCalledWith('orch', 10_000);
    });

    ingestPaneTunnelData(state, 'orch', 'other', Uint8Array.from([1]));
    ingestPaneTunnelData(state, 'orch', 'orch', Uint8Array.from([2]));

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

  it('subscribePaneTunnelData works with explicit pane id before state bind', async () => {
    const state = createPaneTunnelState('mobile');
    const received: Uint8Array[] = [];
    const bridge = {
      readRawBuffer: vi.fn().mockResolvedValue([]),
    } as unknown as Parameters<typeof subscribePaneTunnelData>[1];

    subscribePaneTunnelData(state, bridge, 'orch', (chunk) => received.push(chunk));
    await vi.waitFor(() => {
      expect(bridge.readRawBuffer).toHaveBeenCalledWith('orch', 10_000);
    });
    ingestPaneTunnelData(state, 'orch', 'orch', Uint8Array.from([9]));

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(received).toEqual([Uint8Array.from([9])]);
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
