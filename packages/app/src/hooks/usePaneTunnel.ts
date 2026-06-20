import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { TerminalTransport } from './useTerminalSession';
import type { BridgeClient } from '../lib/bridge';
import {
  bindPaneTunnelSubscribe,
  createPaneTunnelState,
  ingestPaneTunnelData,
  makeDesktopOrchestratorTransport,
  makePaneTunnelTransport,
  mergePaneDimensions,
  setPaneTunnelPane,
  subscribePaneTunnelData,
  type PaneTunnelRole,
} from '../lib/pane-tunnel';
import type { PaneRegistryApi } from './usePaneRegistry';

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

const noopUnsub = () => {};

/**
 * Desktop owns the PTY (Tauri stream + resize). Mobile mirrors over bridge SSE only.
 */
export function usePaneTunnel(
  bridge: BridgeClient | null,
  paneId: string | null | undefined,
  role: PaneTunnelRole,
  registry?: PaneRegistryApi,
): PaneTunnelApi {
  const boundPaneId = paneId ?? null;
  const isDesktop = role === 'desktop';

  const stateRef = useRef(createPaneTunnelState(role));
  stateRef.current.role = role;
  setPaneTunnelPane(stateRef.current, isDesktop ? null : boundPaneId);

  const [dimensions, setDimensions] = useState<{ cols: number; rows: number } | null>(null);

  useEffect(() => {
    if (!isDesktop) {
      setPaneTunnelPane(stateRef.current, boundPaneId);
    }
    setDimensions(null);
  }, [boundPaneId, isDesktop]);

  const registrySubscribePaneData = registry?.subscribePaneData;

  const desktopSubscribe = useCallback(
    (id: string, cb: (data: Uint8Array) => void) => {
      if (!registrySubscribePaneData || !boundPaneId || id !== boundPaneId) return noopUnsub;
      return registrySubscribePaneData(id, cb);
    },
    [registrySubscribePaneData, boundPaneId],
  );

  const mobileSubscribeInner = useCallback(
    (cb: (data: Uint8Array) => void) => {
      if (!bridge || !boundPaneId) return noopUnsub;
      return subscribePaneTunnelData(stateRef.current, bridge, boundPaneId, cb);
    },
    [bridge, boundPaneId],
  );

  const mobileSubscribePaneData = useMemo(() => {
    if (!boundPaneId) {
      return (_paneId: string, _cb: (data: Uint8Array) => void) => noopUnsub;
    }
    return bindPaneTunnelSubscribe(mobileSubscribeInner, boundPaneId);
  }, [boundPaneId, mobileSubscribeInner]);

  const subscribePaneData = isDesktop ? desktopSubscribe : mobileSubscribePaneData;


  const transport = useMemo(() => {
    if (!boundPaneId) return undefined;
    if (isDesktop) return makeDesktopOrchestratorTransport(boundPaneId);
    if (!bridge) return undefined;
    return makePaneTunnelTransport(bridge, boundPaneId);
  }, [boundPaneId, bridge, isDesktop]);

  const ingestTerminalData = useCallback(
    (id: string, data: number[] | Uint8Array) => {
      if (isDesktop) return;
      ingestPaneTunnelData(stateRef.current, boundPaneId, id, data);
    },
    [boundPaneId, isDesktop],
  );

  const updatePaneDimensions = useCallback(
    (id: string, cols: number, rows: number) => {
      if (isDesktop || !boundPaneId || id !== boundPaneId) return;
      setDimensions((prev) => {
        if (prev?.cols === cols && prev?.rows === rows) return prev;
        stateRef.current.streams.reset(boundPaneId);
        return { cols, rows };
      });
    },
    [boundPaneId, isDesktop],
  );

  const mergePaneInfo = useCallback(
    <T extends { cols: number; rows: number }>(info: T | undefined) => {
      if (!info || isDesktop) return info;
      return mergePaneDimensions(info, dimensions?.cols, dimensions?.rows);
    },
    [dimensions, isDesktop],
  );

  return {
    role,
    paneId: boundPaneId,
    subscribePaneData,
    transport,
    ingestTerminalData,
    updatePaneDimensions,
    mergePaneInfo,
  };
}
