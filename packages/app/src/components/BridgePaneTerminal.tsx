import { useMemo } from 'react';
import type { PaneInfo } from '@puppet-master/shared';
import { useDebouncedMirrorLayout } from '../hooks/useDebouncedMirrorLayout';
import { useTerminalSession, type TerminalTransport } from '../hooks/useTerminalSession';
import { isMobileInputDevice } from '../terminal/mobile-input-guard';
import { mirrorLayoutSessionKey } from '../terminal/mirror-layout-session';
import type { TerminalRenderMode } from '../terminal';
import type { PaneDataSubscriber } from '../terminal/types';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-accent',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

interface Props {
  pane: PaneInfo;
  status: PaneInfo['status'];
  subscribePaneData: PaneDataSubscriber;
  transport: TerminalTransport;
  title?: string;
  syncPTYResize?: boolean;
  renderMode?: TerminalRenderMode;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
  /** This phone may not type (paired without it): no input box at all. */
  disableMobileInput?: boolean;
  /** The grid that fits this view (the phone sizes the pane to it). */
  onFitGrid?: (grid: { cols: number; rows: number }) => void;
  /** Phone: false keeps the agent's rows while the keyboard is open (see useTerminalSession). */
  keyboardResizesAgent?: boolean;
  /** Bump `n` to redraw from the PC's current screen (phone nudge after an action). */
  refreshSignal?: { n: number; skipIfScrolledUp: boolean };
}

export function BridgePaneTerminal({
  pane,
  status,
  subscribePaneData,
  transport,
  title,
  syncPTYResize = false,
  renderMode,
  mobileInputDelayMs,
  mobileInputVisible,
  disableMobileInput,
  onFitGrid,
  keyboardResizesAgent,
  refreshSignal,
}: Props) {
  const mobileMirror = !syncPTYResize && isMobileInputDevice();
  const layout = useDebouncedMirrorLayout(pane.cols, pane.rows);
  const effectiveRenderMode =
    renderMode ?? 'mirror-same-grid';
  const { containerRef } = useTerminalSession({
    paneId: pane.id,
    // A phone that sizes the pane resizes this terminal in place: remounting on every size change
    // (keyboard up/down) would destroy the focused input and close the keyboard again.
    sessionKey: onFitGrid
      ? pane.created_at
      : mirrorLayoutSessionKey(
          pane.created_at,
          mobileMirror ? layout.cols : pane.cols,
          mobileMirror ? layout.rows : pane.rows,
        ),
    subscribePaneData,
    transport,
    syncPTYResize,
    renderMode: effectiveRenderMode,
    ptyCols: pane.cols,
    ptyRows: pane.rows,
    mobileInputDelayMs,
    mobileInputVisible,
    disableMobileInput,
    onFitGrid,
    keyboardResizesAgent,
    refreshSignal,
    reprintsOnResize: pane.reprints_on_resize,
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
      <div
        ref={containerRef}
        className={`flex-1 min-h-0 terminal-host overflow-hidden ${effectiveRenderMode === 'mirror-same-grid' ? 'terminal-host--mirror-grid' : ''}`}
      />
    </div>
  );
}

export function useBridgePaneTransport(
  makeTransport: (paneId: string) => TerminalTransport,
  paneId: string,
): TerminalTransport {
  return useMemo(() => makeTransport(paneId), [makeTransport, paneId]);
}
