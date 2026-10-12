// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../lib/bridge';
import { usePaneViewportLease } from './usePaneViewportLease';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Probe(props: { bridge: BridgeClient; paneId: string | null; grid: { cols: number; rows: number } | null }) {
  usePaneViewportLease(props.bridge, props.paneId, props.grid);
  return null;
}

describe('usePaneViewportLease', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('sizes the pane to the phone, renews, follows grid changes and gives it back', async () => {
    const setPaneViewport = vi.fn().mockResolvedValue({});
    const bridge = { setPaneViewport } as unknown as BridgeClient;
    const root = createRoot(document.createElement('div'));

    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={null} />));
    expect(setPaneViewport).not.toHaveBeenCalled();

    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 38 }} />));
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', { cols: 48, rows: 38, active: true });

    await act(async () => vi.advanceTimersByTime(5000));
    expect(setPaneViewport).toHaveBeenCalledTimes(2);

    // Keyboard opened: fewer rows, claimed straight away with no release in between.
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 20 }} />));
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', { cols: 48, rows: 20, active: true });
    expect(setPaneViewport.mock.calls.some(([, view]) => view.active === false)).toBe(false);

    // Back to All: the pane goes back to its desktop size.
    await act(async () => root.render(<Probe bridge={bridge} paneId={null} grid={{ cols: 48, rows: 20 }} />));
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', { cols: 0, rows: 0, active: false });
    await act(async () => root.unmount());
  });
});
