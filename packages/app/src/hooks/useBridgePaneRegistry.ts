import { useCallback, useEffect, useRef, useState } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { PaneStreamManager } from '../terminal';
import type { PaneDataListener, PaneDataSubscriber, PaneResizeListener } from '../terminal/types';
import type { BridgeClient } from '../lib/bridge';
import { makePaneTunnelTransport } from '../lib/pane-tunnel';
import type { TerminalTransport } from './useTerminalSession';

export interface BridgePaneData {
  info: PaneInfo;
  status: PaneInfo['status'];
}

export interface BridgePaneRegistryApi {
  panes: Map<string, BridgePaneData>;
  paneList: BridgePaneData[];
  subscribePaneData: PaneDataSubscriber;
  /** A `pane-resize` event, passed to the open terminal in order with the pane's output. */
  ingestPaneResize: (paneId: string, cols: number, rows: number, offset?: number) => void;
  /** `offset`: where the chunk starts in the pane's byte stream (`terminal` event), when the backend sends it. */
  ingestTerminalData: (paneId: string, data: number[] | Uint8Array, offset?: number) => void;
  updatePaneStatus: (paneId: string, status: PaneInfo['status']) => void;
  /**
   * `keepStream`: the viewer resizes its terminal in place (phone-sized panes) and keeps reading
   * the live stream; without it the stream is reset so a remounted mirror replays the scrollback.
   */
  updatePaneDimensions: (paneId: string, cols: number, rows: number, keepStream?: boolean) => void;
  setPanesFromList: (list: PaneInfo[]) => void;
  /** Forget every live stream so the next subscriber replays the pane's scrollback (missed events). */
  resetStreams: () => void;
  makeTransport: (paneId: string) => TerminalTransport;
}

const EMPTY: Map<string, BridgePaneData> = new Map();

/** Socket input sender (see BridgeEventsHandle.sendInput); returns false when the socket is not open. */
export type SocketInputSender = (paneId: string, text: string, appendNewline: boolean) => boolean;

export function useBridgePaneRegistry(
  bridge: BridgeClient | null,
  sendViaSocket?: SocketInputSender,
  /** Output went missing in transit (offsets show a hole): the viewer should redraw from the PC. */
  onStreamGap?: (paneId: string) => void,
): BridgePaneRegistryApi {
  const [panes, setPanes] = useState<Map<string, BridgePaneData>>(EMPTY);
  const panesRef = useRef(panes);
  panesRef.current = panes;
  const onStreamGapRef = useRef(onStreamGap);
  onStreamGapRef.current = onStreamGap;
  const streamsRef = useRef<PaneStreamManager | null>(null);
  if (!streamsRef.current) {
    streamsRef.current = new PaneStreamManager({ onGap: (paneId) => onStreamGapRef.current?.(paneId) });
  }

  const setPanesFromList = useCallback((list: PaneInfo[]) => {
    const next = new Map<string, BridgePaneData>();
    for (const info of list) {
      next.set(info.id, { info, status: info.status });
    }
    setPanes(next);
  }, []);

  const ingestTerminalData = useCallback((paneId: string, data: number[] | Uint8Array, offset?: number) => {
    streamsRef.current!.ingest(paneId, data, offset);
  }, []);

  const updatePaneStatus = useCallback((paneId: string, status: PaneInfo['status']) => {
    setPanes((prev) => {
      const next = new Map(prev);
      const existing = next.get(paneId);
      if (existing) {
        next.set(paneId, { ...existing, status });
      }
      return next;
    });
  }, []);

  const updatePaneDimensions = useCallback((paneId: string, cols: number, rows: number, keepStream = false) => {
    setPanes((prev) => {
      const existing = prev.get(paneId);
      if (!existing) return prev;
      if (existing.info.cols === cols && existing.info.rows === rows) return prev;
      if (!keepStream) streamsRef.current!.reset(paneId);
      const next = new Map(prev);
      next.set(paneId, {
        ...existing,
        info: { ...existing.info, cols, rows },
      });
      return next;
    });
  }, []);

  const subscribePaneData = useCallback(
    (paneId: string, cb: PaneDataListener, onResize?: PaneResizeListener) => {
      if (!bridge) return () => {};
      // Phone-sized terminals (onResize) replay the pane redrawn at its current size; the raw
      // history was written for other sizes. An older desktop build without /screen: raw replay.
      const replay = onResize
        ? (id: string, lines: number) => bridge.readScreen(id).catch(() => bridge.readRawBuffer(id, lines))
        : (id: string, lines: number) => bridge.readRawBuffer(id, lines);
      return streamsRef.current!.subscribe(paneId, cb, replay, onResize);
    },
    [bridge],
  );

  const ingestPaneResize = useCallback((paneId: string, cols: number, rows: number, offset?: number) => {
    streamsRef.current!.ingestResize(paneId, cols, rows, offset);
  }, []);

  const resetStreams = useCallback(() => {
    for (const paneId of panesRef.current.keys()) streamsRef.current!.reset(paneId);
  }, []);

  const makeTransport = useCallback(
    (paneId: string) => makePaneTunnelTransport(bridge!, paneId, sendViaSocket),
    [bridge, sendViaSocket],
  );

  useEffect(() => {
    if (!bridge) return;
    void bridge.listPanes().then(setPanesFromList).catch(() => {});
  }, [bridge, setPanesFromList]);

  const paneList = Array.from(panes.values()).sort((a, b) => a.info.created_at - b.info.created_at);

  return {
    panes,
    paneList,
    subscribePaneData,
    ingestPaneResize,
    ingestTerminalData,
    updatePaneStatus,
    updatePaneDimensions,
    setPanesFromList,
    resetStreams,
    makeTransport,
  };
}
