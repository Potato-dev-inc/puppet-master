import { useTerminalSession, type TerminalTransport } from '../hooks/useTerminalSession';
import { useDebouncedMirrorLayout } from '../hooks/useDebouncedMirrorLayout';
import type { PaneData } from '../hooks/usePaneRegistry';
import { BACKEND_LABEL, type CliOrchestratorBackend } from '../lib/orchestrator-panes';
import { isMobileInputDevice } from '../terminal/mobile-input-guard';
import { mirrorLayoutSessionKey } from '../terminal/mirror-layout-session';
import type { TerminalRenderMode } from '../terminal';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-accent',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

interface Props {
  backend: CliOrchestratorBackend;
  pane: PaneData | undefined;
  starting: boolean;
  error: string | null;
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  onRetry: () => void;
  transport?: TerminalTransport;
  syncPTYResize?: boolean;
  renderMode?: TerminalRenderMode;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
}

function OrchestratorTerminalLive({
  backend,
  pane,
  subscribePaneData,
  transport,
  syncPTYResize = true,
  renderMode,
  mobileInputDelayMs,
  mobileInputVisible,
}: {
  backend: CliOrchestratorBackend;
  pane: PaneData;
  subscribePaneData: (paneId: string, cb: (data: Uint8Array) => void) => () => void;
  transport?: TerminalTransport;
  syncPTYResize?: boolean;
  renderMode?: TerminalRenderMode;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
}) {
  const mobileMirror = !syncPTYResize && isMobileInputDevice();
  const layout = useDebouncedMirrorLayout(pane.info.cols, pane.info.rows);
  const effectiveRenderMode =
    renderMode ?? (mobileMirror ? 'mirror-same-grid' : undefined);
  const { containerRef } = useTerminalSession({
    paneId: pane.info.id,
    sessionKey: syncPTYResize
      ? pane.info.created_at
      : mirrorLayoutSessionKey(
          pane.info.created_at,
          layout.cols,
          layout.rows,
        ),
    subscribePaneData,
      transport,
    syncPTYResize,
    renderMode: effectiveRenderMode,
    ptyCols: pane.info.cols,
    ptyRows: pane.info.rows,
    mobileInputDelayMs,
    mobileInputVisible,
  });
  const label = BACKEND_LABEL[backend];

  return (
    <div className="flex-1 min-h-0 flex flex-col overflow-hidden">
      <div className="flex items-center gap-2 px-2 py-1 border-b border-pm-border bg-pm-bg/60 text-xs text-pm-text">
        <span
          className={`inline-block w-2 h-2 rounded-full ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`}
          title={pane.status}
        />
        <span className="font-medium">{label}</span>
        <span className="text-pm-muted truncate">orchestrator · pid {pane.info.pid}</span>
      </div>
      <div
        ref={containerRef}
        className={`flex-1 min-h-0 terminal-host overflow-hidden ${effectiveRenderMode === 'mirror-same-grid' ? 'terminal-host--mirror-grid' : ''}`}
      />
    </div>
  );
}

export function OrchestratorTerminal({
  backend,
  pane,
  starting,
  error,
  subscribePaneData,
  onRetry,
  transport,
  syncPTYResize = true,
  renderMode,
  mobileInputDelayMs,
  mobileInputVisible,
}: Props) {
  const label = BACKEND_LABEL[backend];

  if (error) {
    return (
      <div className="flex-1 min-h-0 flex flex-col items-center justify-center gap-2 p-4 text-xs text-center">
        <div className="text-pm-err font-medium">Failed to start {label}</div>
        <div className="text-pm-muted">{error}</div>
        <button
          onClick={onRetry}
          className="px-2 py-1 rounded border border-pm-border hover:bg-pm-border/40"
        >
          Retry
        </button>
      </div>
    );
  }

  if (!pane) {
    return (
      <div className="flex-1 min-h-0 flex items-center justify-center text-xs text-pm-muted">
        {starting ? `Starting ${label} orchestrator…` : `Connecting to ${label}…`}
      </div>
    );
  }

  return (
    <OrchestratorTerminalLive
      key={`${backend}:${pane.info.id}:${pane.info.created_at}`}
      backend={backend}
      pane={pane}
      subscribePaneData={subscribePaneData}
      transport={transport}
      syncPTYResize={syncPTYResize}
      renderMode={renderMode}
      mobileInputDelayMs={mobileInputDelayMs}
      mobileInputVisible={mobileInputVisible}
    />
  );
}
