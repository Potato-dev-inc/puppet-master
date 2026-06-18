import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { TerminalTransport } from './useTerminalSession';
import type { BridgeClient } from '../lib/bridge';
import {
  bindPaneTunnelSubscribe,
  createPaneTunnelState,
  ingestPaneTunnelData,
  makePaneTunnelTransport,
  mergePaneDimensions,
  setPaneTunnelPane,
  subscribePaneTunnelData,
  type PaneTunnelRole,
} from '../lib/pane-tunnel';

export interface PaneTunnelApi {
  role: PaneTunnelRole;
  paneId: string | null;
  /** Bridge mirror: subscribe to live PTY bytes for this pane only. */
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  transport: TerminalTransport | undefined;
  ingestTerminalData: (paneId: string, data: number[] | Uint8Array) => void;
  updatePaneDimensions: (paneId: string, cols: number, rows: number) => void;
  /** Pane info with tunnel-tracked cols/rows merged in. */
  mergePaneInfo: <T extends { cols: number; rows: number }>(info: T | undefined) => T | undefined;
}

/**
 * One bridge mirror tunnel for a single pane. Desktop and mobile orchestrator
 * each get their own instance; both use the same PTY stream over SSE + bridge input.
 */
export function usePaneTunnel(
  bridge: BridgeClient | null,
  paneId: string | null | undefined,
  role: PaneTunnelRole,
): PaneTunnelApi {
  const stateRef = useRef(createPaneTunnelState(role));
  stateRef.current.role = role;
  const [dimensions, setDimensions] = useState<{ cols: number; rows: number } | null>(null);

  useEffect(() => {
    setPaneTunnelPane(stateRef.current, paneId ?? null);
    setDimensions(null);
  }, [paneId]);

  const ingestTerminalData = useCallback((id: string, data: number[] | Uint8Array) => {
    ingestPaneTunnelData(stateRef.current, id, data);
  }, []);

  const updatePaneDimensions = useCallback(
    (id: string, cols: number, rows: number) => {
      if (!paneId || id !== paneId) return;
      setDimensions((prev) => {
        if (prev?.cols === cols && prev?.rows === rows) return prev;
        return { cols, rows };
      });
    },
    [paneId],
  );

  const subscribe = useCallback(
    (cb: (data: Uint8Array) => void) => {
      if (!bridge) return () => {};
      return subscribePaneTunnelData(stateRef.current, bridge, cb);
    },
    [bridge],
  );

  const subscribePaneData = useMemo(() => {
    if (!paneId) {
      return (_paneId: string, _cb: (data: Uint8Array) => void) => () => {};
    }
    return bindPaneTunnelSubscribe(subscribe, paneId);
  }, [paneId, subscribe]);

  const transport = useMemo(() => {
    if (!bridge || !paneId) return undefined;
    return makePaneTunnelTransport(bridge, paneId);
  }, [bridge, paneId]);

  const mergePaneInfo = useCallback(
    <T extends { cols: number; rows: number }>(info: T | undefined) => {
      if (!info) return undefined;
      return mergePaneDimensions(info, dimensions?.cols, dimensions?.rows);
    },
    [dimensions],
  );

  return {
    role,
    paneId: paneId ?? null,
    subscribePaneData,
    transport,
    ingestTerminalData,
    updatePaneDimensions,
    mergePaneInfo,
  };
}
