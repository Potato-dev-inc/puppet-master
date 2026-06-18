import type { BridgeEvent } from './bridge';
import type { PaneTunnelApi } from '../hooks/usePaneTunnel';

/** Feed bridge terminal SSE events into a pane tunnel (orchestrator mirror viewers). */
export function routeBridgeEventToPaneTunnel(ev: BridgeEvent, tunnel: PaneTunnelApi): void {
  switch (ev.type) {
    case 'terminal':
      tunnel.ingestTerminalData(ev.pane_id, ev.data);
      break;
    case 'pane-resize':
      tunnel.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows);
      break;
    default:
      break;
  }
}
