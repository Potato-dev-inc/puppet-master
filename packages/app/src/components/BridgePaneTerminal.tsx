import { useMemo } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { useTerminalSession, type TerminalTransport } from '../hooks/useTerminalSession';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-accent',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

interface Props {
  pane: PaneInfo;
  status: PaneInfo['status'];
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  transport: TerminalTransport;
  title?: string;
  syncPTYResize?: boolean;
}

export function BridgePaneTerminal({
  pane,
  status,
  subscribePaneData,
  transport,
  title,
  syncPTYResize = false,
}: Props) {
  const containerRef = useTerminalSession({
    paneId: pane.id,
    sessionKey: pane.created_at,
    subscribePaneData,
    transport,
    syncPTYResize,
    ptyCols: pane.cols,
    ptyRows: pane.rows,
  });

  const label = title ?? pane.agent_type;

  return (
    <div className="flex-1 min-h-0 flex flex-col overflow-hidden">
      <div className="flex items-center gap-2 px-2 py-1 border-b border-pm-border bg-pm-bg/60 text-xs text-pm-text shrink-0">
        <span
          className={`inline-block w-2 h-2 rounded-full ${STATUS_COLOR[status] ?? 'bg-pm-muted'}`}
          title={status}
        />
        <span className="font-medium truncate">{label}</span>
        <span className="text-pm-muted truncate">
          {pane.cols}×{pane.rows} · pid {pane.pid}
        </span>
      </div>
      <div ref={containerRef} className="flex-1 min-h-0 overflow-auto terminal-host" />
    </div>
  );
}

export function useBridgePaneTransport(
  makeTransport: (paneId: string) => TerminalTransport,
  paneId: string,
): TerminalTransport {
  return useMemo(() => makeTransport(paneId), [makeTransport, paneId]);
}
