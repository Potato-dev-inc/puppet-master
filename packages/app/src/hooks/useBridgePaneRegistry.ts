import { useCallback, useEffect, useRef, useState } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { PaneStreamManager } from '../terminal';
import type { BridgeClient } from '../lib/bridge';
import { makePaneTunnelTransport } from '../lib/pane-tunnel';

export interface BridgePaneData {
  info: PaneInfo;
  status: PaneInfo['status'];
}

export interface BridgePaneRegistryApi {
  panes: Map<string, BridgePaneData>;
  paneList: BridgePaneData[];
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  ingestTerminalData: (paneId: string, data: number[] | Uint8Array) => void;
  updatePaneStatus: (paneId: string, status: PaneInfo['status']) => void;
  updatePaneDimensions: (paneId: string, cols: number, rows: number) => void;
  setPanesFromList: (list: PaneInfo[]) => void;
  makeTransport: (paneId: string) => {
    resize: (cols: number, rows: number) => Promise<void>;
    writeInput: (text: string, appendNewline?: boolean) => Promise<void>;
  };
}

const EMPTY: Map<string, BridgePaneData> = new Map();

export function useBridgePaneRegistry(bridge: BridgeClient | null): BridgePaneRegistryApi {
  const [panes, setPanes] = useState<Map<string, BridgePaneData>>(EMPTY);
  const streamsRef = useRef(new PaneStreamManager());

  const setPanesFromList = useCallback((list: PaneInfo[]) => {
    const next = new Map<string, BridgePaneData>();
    for (const info of list) {
      next.set(info.id, { info, status: info.status });
    }
    setPanes(next);
  }, []);

  const ingestTerminalData = useCallback((paneId: string, data: number[] | Uint8Array) => {
    streamsRef.current.ingest(paneId, data);
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

  const updatePaneDimensions = useCallback((paneId: string, cols: number, rows: number) => {
    setPanes((prev) => {
      const next = new Map(prev);
      const existing = next.get(paneId);
      if (existing) {
        next.set(paneId, {
          ...existing,
          info: { ...existing.info, cols, rows },
        });
      }
      return next;
    });
  }, []);

  const subscribePaneData = useCallback(
    (paneId: string, cb: (data: Uint8Array) => void) => {
      if (!bridge) return () => {};
      return streamsRef.current.subscribe(paneId, cb, (id, lines) => bridge.readRawBuffer(id, lines));
    },
    [bridge],
  );

  const makeTransport = useCallback(
    (paneId: string) => makePaneTunnelTransport(bridge!, paneId),
    [bridge],
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
    ingestTerminalData,
    updatePaneStatus,
    updatePaneDimensions,
    setPanesFromList,
    makeTransport,
  };
}
