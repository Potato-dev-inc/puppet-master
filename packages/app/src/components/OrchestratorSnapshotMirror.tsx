import type { PaneData } from '../hooks/usePaneRegistry';
import { useOrchestratorSnapshotMirror } from '../hooks/useOrchestratorSnapshotMirror';
import type { TerminalTransport } from '../hooks/useTerminalSession';
import type { BridgeClient } from '../lib/bridge';
import { BACKEND_LABEL, type CliOrchestratorBackend } from '../lib/orchestrator-panes';

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
  bridge: BridgeClient;
  subscribeSnapshots: (paneId: string, cb: (snapshot: string) => void) => () => void;
  onRetry: () => void;
  transport?: TerminalTransport;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
}

function OrchestratorSnapshotMirrorLive({
  backend,
  pane,
  bridge,
  subscribeSnapshots,
  transport,
  mobileInputDelayMs,
  mobileInputVisible,
}: {
  backend: CliOrchestratorBackend;
  pane: PaneData;
  bridge: BridgeClient;
  subscribeSnapshots: (paneId: string, cb: (snapshot: string) => void) => () => void;
  transport?: TerminalTransport;
  mobileInputDelayMs?: number;
  mobileInputVisible?: boolean;
}) {
  const containerRef = useOrchestratorSnapshotMirror({
    paneId: pane.info.id,
    sessionKey: pane.info.created_at,
    bridge,
    transport,
    subscribeSnapshots,
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
      <div ref={containerRef} className="flex-1 min-h-0 overflow-hidden terminal-host overflow-auto" />
    </div>
  );
}

export function OrchestratorSnapshotMirror({
  backend,
  pane,
  starting,
  error,
  bridge,
  subscribeSnapshots,
  onRetry,
  transport,
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
    <OrchestratorSnapshotMirrorLive
      backend={backend}
      pane={pane}
      bridge={bridge}
      subscribeSnapshots={subscribeSnapshots}
      transport={transport}
      mobileInputDelayMs={mobileInputDelayMs}
      mobileInputVisible={mobileInputVisible}
    />
  );
}
