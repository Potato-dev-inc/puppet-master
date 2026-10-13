// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { BridgeClient } from '../lib/bridge';
import { usePaneViewportLease } from './usePaneViewportLease';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Grid = { cols: number; rows: number } | null;

function Probe(props: {
  bridge: BridgeClient;
  paneId: string | null;
  grid: Grid;
  claim: { current: (() => void) | null };
}) {
  props.claim.current = usePaneViewportLease(props.bridge, props.paneId, props.grid);
  return null;
}

const releases = (spy: ReturnType<typeof vi.fn>) =>
  spy.mock.calls.filter(([, view]) => view.active === false);
const renewals = (spy: ReturnType<typeof vi.fn>) =>
  spy.mock.calls.filter(([, view]) => view.renew_only === true);

describe('usePaneViewportLease', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('does not resize on view; phone input takes the size, renews it and follows grid changes', async () => {
    const setPaneViewport = vi.fn().mockResolvedValue({ held: true });
    const bridge = { setPaneViewport } as unknown as BridgeClient;
    const root = createRoot(document.createElement('div'));
    const claim: { current: (() => void) | null } = { current: null };

    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={null} claim={claim} />));
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 38 }} claim={claim} />));
    await act(async () => vi.advanceTimersByTime(15000));
    expect(setPaneViewport).not.toHaveBeenCalled();

    await act(async () => claim.current?.());
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', expect.objectContaining({ cols: 48, rows: 38, active: true }));
    expect(setPaneViewport).toHaveBeenCalledTimes(1);

    // Typing again with the same grid costs no request.
    await act(async () => claim.current?.());
    expect(setPaneViewport).toHaveBeenCalledTimes(1);

    await act(async () => vi.advanceTimersByTime(5000));
    expect(renewals(setPaneViewport)).toHaveLength(1);
    expect(renewals(setPaneViewport)[0][1]).toEqual(expect.objectContaining({ cols: 48, rows: 38, active: true }));

    // Keyboard opened while holding: re-claimed with the new grid, no release in between.
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 20 }} claim={claim} />));
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', expect.objectContaining({ cols: 48, rows: 20, active: true }));
    expect(releases(setPaneViewport)).toHaveLength(0);

    // Leaving the tab releases the hold (the bridge keeps the size).
    await act(async () => root.render(<Probe bridge={bridge} paneId={null} grid={{ cols: 48, rows: 20 }} claim={claim} />));
    expect(releases(setPaneViewport)).toHaveLength(1);
    expect(setPaneViewport).toHaveBeenLastCalledWith('p1', expect.objectContaining({ cols: 0, rows: 0, active: false }));
    await act(async () => root.unmount());
  });

  it('does not claim on view, and renewal stops once the desktop took the size back', async () => {
    const setPaneViewport = vi
      .fn()
      .mockResolvedValueOnce({ held: true })
      .mockResolvedValueOnce({ held: false })
      .mockResolvedValue({ held: false });
    const bridge = { setPaneViewport } as unknown as BridgeClient;
    const root = createRoot(document.createElement('div'));
    const claim: { current: (() => void) | null } = { current: null };
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 38 }} claim={claim} />));

    await act(async () => claim.current?.());
    await act(async () => vi.advanceTimersByTime(5000));
    expect(renewals(setPaneViewport)).toHaveLength(1);

    // The desktop took it back (held: false): no more renewals.
    await act(async () => vi.advanceTimersByTime(20000));
    expect(renewals(setPaneViewport)).toHaveLength(1);

    // The next keystroke claims again.
    await act(async () => claim.current?.());
    expect(setPaneViewport).toHaveBeenCalledTimes(3);
    expect(setPaneViewport.mock.calls[2][1]).toEqual(expect.objectContaining({ active: true }));
    expect(setPaneViewport.mock.calls[2][1].renew_only).toBeUndefined();
    await act(async () => root.unmount());
  });

  it('keeps the hold through a brief hide and releases it after a long one', async () => {
    const setPaneViewport = vi.fn().mockResolvedValue({ held: true });
    const bridge = { setPaneViewport } as unknown as BridgeClient;
    const root = createRoot(document.createElement('div'));
    const claim: { current: (() => void) | null } = { current: null };
    let state: DocumentVisibilityState = 'visible';
    const spy = vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => state);
    const flip = (next: DocumentVisibilityState) => {
      state = next;
      document.dispatchEvent(new Event('visibilitychange'));
    };
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 38 }} claim={claim} />));
    await act(async () => claim.current?.());

    await act(async () => {
      flip('hidden');
      vi.advanceTimersByTime(1000);
      flip('visible');
      vi.advanceTimersByTime(5000);
    });
    expect(releases(setPaneViewport)).toHaveLength(0);
    expect(renewals(setPaneViewport).length).toBeGreaterThan(0);

    await act(async () => {
      flip('hidden');
      vi.advanceTimersByTime(3000);
    });
    expect(releases(setPaneViewport)).toHaveLength(1);
    const callsAfterRelease = setPaneViewport.mock.calls.length;
    await act(async () => vi.advanceTimersByTime(15000));
    expect(setPaneViewport.mock.calls.length).toBe(callsAfterRelease);

    await act(async () => root.unmount());
    spy.mockRestore();
  });

  it('never takes the size on becoming visible', async () => {
    const setPaneViewport = vi.fn().mockResolvedValue({ held: true });
    const bridge = { setPaneViewport } as unknown as BridgeClient;
    const root = createRoot(document.createElement('div'));
    const claim: { current: (() => void) | null } = { current: null };
    let state: DocumentVisibilityState = 'hidden';
    const spy = vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => state);
    await act(async () => root.render(<Probe bridge={bridge} paneId="p1" grid={{ cols: 48, rows: 38 }} claim={claim} />));
    state = 'visible';
    await act(async () => {
      document.dispatchEvent(new Event('visibilitychange'));
      vi.advanceTimersByTime(10000);
    });
    expect(setPaneViewport).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    spy.mockRestore();
  });
});
