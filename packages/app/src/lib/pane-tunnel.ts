import { PaneStreamManager } from '../terminal';
import type { TerminalTransport } from '../hooks/useTerminalSession';
import type { BridgeClient } from './bridge';

/** Shared bridge mirror transport — viewers must not resize the backend PTY. */
export function makePaneTunnelTransport(bridge: BridgeClient, paneId: string): TerminalTransport {
  return {
    resize: async () => {
      void paneId;
    },
    writeInput: async (text: string, appendNewline = false) => {
      await bridge.writeInput(paneId, text, appendNewline);
    },
  };
}

export type PaneTunnelRole = 'desktop' | 'mobile';

export interface PaneTunnelState {
  role: PaneTunnelRole;
  paneId: string | null;
  streams: PaneStreamManager;
}

export function createPaneTunnelState(role: PaneTunnelRole): PaneTunnelState {
  return {
    role,
    paneId: null,
    streams: new PaneStreamManager(),
  };
}

export function setPaneTunnelPane(state: PaneTunnelState, paneId: string | null): void {
  if (state.paneId === paneId) return;
  if (state.paneId) {
    state.streams.reset(state.paneId);
  }
  state.paneId = paneId;
}

export function ingestPaneTunnelData(
  state: PaneTunnelState,
  paneId: string,
  data: number[] | Uint8Array,
): void {
  if (!state.paneId || paneId !== state.paneId) return;
  state.streams.ingest(paneId, data);
}

export function subscribePaneTunnelData(
  state: PaneTunnelState,
  bridge: BridgeClient,
  cb: (data: Uint8Array) => void,
): () => void {
  if (!state.paneId) return () => {};
  return state.streams.subscribe(state.paneId, cb, (id, lines) => bridge.readRawBuffer(id, lines));
}

/** Adapter for components that expect `(paneId, cb) => unsub`. */
export function bindPaneTunnelSubscribe(
  subscribe: (cb: (data: Uint8Array) => void) => () => void,
  boundPaneId: string,
): (paneId: string, cb: (data: Uint8Array) => void) => () => void {
  return (paneId, cb) => {
    if (paneId !== boundPaneId) return () => {};
    return subscribe(cb);
  };
}

export function mergePaneDimensions<T extends { cols: number; rows: number }>(
  info: T | undefined,
  cols: number | undefined,
  rows: number | undefined,
): T | undefined {
  if (!info || cols === undefined || rows === undefined) return info;
  if (info.cols === cols && info.rows === rows) return info;
  return { ...info, cols, rows };
}
