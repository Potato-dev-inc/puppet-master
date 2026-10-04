// "Tools" tab: coordination panel above a drag-resizable MCP call log.
import type { PointerEvent } from 'react';
import type { BridgeClient } from '../../lib/bridge';
import { CoordinationPanel } from '../CoordinationPanel';
import type { UiLogEntry } from './chat-model';

interface Props {
  bridge: BridgeClient | null;
  bridgeReady: boolean;
  logs: UiLogEntry[];
  /** Owned by the parent so the chosen height survives switching tabs. */
  logHeight: number;
  onLogHeightChange: (height: number) => void;
}

const MIN_LOG_HEIGHT = 72;
const MAX_LOG_HEIGHT = 280;

export function ToolsView({ bridge, bridgeReady, logs, logHeight, onLogHeightChange }: Props) {
  const resizeMcpLog = (event: PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = logHeight;
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);

    const move = (moveEvent: globalThis.PointerEvent) => {
      const delta = startY - moveEvent.clientY;
      onLogHeightChange(Math.min(MAX_LOG_HEIGHT, Math.max(MIN_LOG_HEIGHT, startHeight + delta)));
    };
    const up = (upEvent: globalThis.PointerEvent) => {
      target.releasePointerCapture(upEvent.pointerId);
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
      target.removeEventListener('pointercancel', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
    target.addEventListener('pointercancel', up);
  };

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <div className="flex-1 min-h-0 overflow-auto">
        <CoordinationPanel bridge={bridge} bridgeReady={bridgeReady} />
      </div>

      <div
        role="separator"
        aria-orientation="horizontal"
        title="Resize MCP log"
        className="h-1 shrink-0 cursor-row-resize bg-pm-border/60 hover:bg-pm-accent transition-colors touch-none"
        onPointerDown={resizeMcpLog}
      />

      <div
        className="border-t border-pm-border overflow-auto shrink-0"
        style={{ height: logHeight }}
      >
        <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-pm-muted border-b border-pm-border bg-pm-bg/40">
          MCP log
        </div>
        {logs.length === 0 ? (
          <div className="px-3 py-1 text-[10px] text-pm-muted">No tool calls yet.</div>
        ) : (
          logs.slice(0, 30).map((l) => (
            <div key={l.id} className="px-3 py-0.5 text-[10px] font-mono">
              <span className="text-pm-muted">[{l.source}]</span>{' '}
              <span className={l.error ? 'text-pm-err' : 'text-pm-accent'}>{l.tool}</span>
              {l.error && <span className="text-pm-err"> — {l.error}</span>}
              {!l.error && l.result && (
                <span className="text-pm-muted"> → {l.result.slice(0, 80)}</span>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
