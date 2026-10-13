// One terminal pane as used by the standalone / detached pane window: xterm host, optional title bar,
// and floating controls (move back to app, API key tray, reflow nudge).
import { useTerminalSession } from '../hooks/useTerminalSession';
import { agentLabel, useAgentRegistry } from '../lib/agentRegistry';
import type { PaneData, PaneRegistryApi } from '../hooks/usePaneRegistry';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-warn',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

function paneTitle(agentType: string, registryLabel: (id: string) => string): string {
  switch (agentType) {
    case 'cmd':
      return 'Command Prompt';
    case 'powershell':
      return 'Terminal (PowerShell)';
    case 'bash':
      return 'Terminal (Shell)';
    case 'claude':
      return 'Claude Code';
    case 'codex':
      return 'Codex CLI';
    case 'opencode':
      return 'OpenCode';
    case 'opencode_native':
      return 'OpenCode (API)';
    default:
      return registryLabel(agentType);
  }
}

export function TerminalViewport({
  pane,
  registry,
  chrome = true,
  disableMobileInput = false,
  onReattach,
  reflowKey,
  workerHost = false,
  workerTrayOpen = false,
  onToggleWorkerTray,
}: {
  pane: PaneData;
  registry: PaneRegistryApi;
  chrome?: boolean;
  disableMobileInput?: boolean;
  onReattach?: () => void;
  reflowKey?: number | string;
  workerHost?: boolean;
  workerTrayOpen?: boolean;
  onToggleWorkerTray?: () => void;
}) {
  const listing = useAgentRegistry();
  const { containerRef, nudgeReflow } = useTerminalSession({
    paneId: pane.info.id,
    sessionKey: pane.info.created_at,
    subscribePaneData: registry.subscribePaneData,
    syncPTYResize: true,
    disableMobileInput,
    reflowKey,
    heldPtySize: pane.info.viewer_held ? { cols: pane.info.cols, rows: pane.info.rows } : null,
  });

  return (
    <section
      className={chrome ? 'pm-terminal-stage' : 'pm-terminal-stage pm-terminal-stage--bare'}
      aria-label="Command terminal"
    >
      {chrome && (
        <div className="pm-terminal-titlebar">
          <span className={`pm-terminal-status ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`} />
          <strong>{paneTitle(pane.info.agent_type, (id) => agentLabel(listing, id))}</strong>
          <span>pid {pane.info.pid}</span>
          <span>{pane.info.cols}x{pane.info.rows}</span>
        </div>
      )}
      <div ref={containerRef} className="terminal-host pm-terminal-host" />
      {pane.info.viewer_held && (
        // The PTY is sized to a phone that is viewing this pane: this window's grid does not match,
        // so say so instead of drawing a broken (often black) screen. Ends when the phone leaves.
        <div className="pm-terminal-viewer-held" role="status">
          <strong>Sized for your phone</strong>
          <span>
            {pane.info.cols}×{pane.info.rows} while the phone has this agent open. This window comes back when
            you leave the tab on the phone.
          </span>
        </div>
      )}
      {!chrome && (onReattach || workerHost || onToggleWorkerTray) && (
        <div className="pm-terminal-floating-controls">
          {onReattach && (
            <button
              type="button"
              className="pm-terminal-control-button"
              title="Move pane back to app"
              aria-label="Move pane back to app"
              onClick={onReattach}
            >
              ↙
            </button>
          )}
          {workerHost && pane.info.agent_type === 'opencode_native' && onToggleWorkerTray && (
            <button
              type="button"
              className={`pm-terminal-control-button pm-terminal-control-button--tray${workerTrayOpen ? ' is-open' : ''}`}
              title={workerTrayOpen ? 'Hide API key setup' : 'Open API key setup'}
              aria-label={workerTrayOpen ? 'Hide API key setup' : 'Open API key setup'}
              aria-expanded={workerTrayOpen}
              onClick={onToggleWorkerTray}
            >
              ˄
            </button>
          )}
          <button
            type="button"
            className="pm-terminal-control-button"
            title="Nudge layout reflow"
            aria-label="Nudge layout reflow"
            onClick={nudgeReflow}
          >
            ↻
          </button>
        </div>
      )}
    </section>
  );
}
