import type { BridgeEvent } from './bridge';
import type { PaneTunnelApi } from '../hooks/usePaneTunnel';

/** Feed bridge terminal and resize SSE into the mobile mirror tunnel only. */
export function routeBridgeEventToPaneTunnel(ev: BridgeEvent, tunnel: PaneTunnelApi): void {
  if (tunnel.role !== 'mobile') return;
  if (ev.type === 'terminal') {
    tunnel.ingestTerminalData(ev.pane_id, ev.data);
  } else if (ev.type === 'pane-resize') {
    tunnel.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows);
  }
}
