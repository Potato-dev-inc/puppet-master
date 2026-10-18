// One terminal pane as used by the standalone / detached pane window: xterm host, optional title bar,
// and floating controls (move back to app, API key tray, reflow nudge).
import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react';
import { useTerminalSession } from '../hooks/useTerminalSession';
import { agentLabel, useAgentRegistry } from '../lib/agentRegistry';
import { tauri } from '../lib/tauri';
import { nudgeDesktop, onDesktopNudge } from '../lib/desktop-nudge';
import { NUDGE_MIN_GAP_MS } from '../lib/nudge-scheduler';
import type { PaneData, PaneRegistryApi } from '../hooks/usePaneRegistry';

/** Pointer, key and focus events in a phone-held window come in bursts: one reclaim per window. */
const RECLAIM_THROTTLE_MS = 500;

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
  redrawKey,
  workerHost = false,
  workerTrayOpen = false,
  onToggleWorkerTray,
  getSelectionRef,
}: {
  pane: PaneData;
  registry: PaneRegistryApi;
  chrome?: boolean;
  disableMobileInput?: boolean;
  onReattach?: () => void;
  /** Bump to re-fit at the current size (sends no PTY resize when the grid is unchanged). */
  reflowKey?: number | string;
  /** Bump to also force an alternate-screen TUI to redraw (two PTY resizes): an explicit Reflow only. */
  redrawKey?: number | string;
  workerHost?: boolean;
  workerTrayOpen?: boolean;
  onToggleWorkerTray?: () => void;
  /** Optional: receives a function returning the terminal's current selection text. */
  getSelectionRef?: MutableRefObject<(() => string) | null>;
}) {
  const listing = useAgentRegistry();
  // Desktop nudge: rebuild the terminal exactly as a fresh mount does (new xterm, fit, screen replay
  // at the PTY's size, live). Bumping the epoch re-runs the session effect; nothing else differs.
  const [epoch, setEpoch] = useState(0);
  const mountedAtRef = useRef(Date.now());
  const { containerRef, afterReclaim, isScrolledUp, getSelection } = useTerminalSession({
    remountEpoch: epoch,
    assumePtyGrid: { cols: pane.info.cols, rows: pane.info.rows },
    paneId: pane.info.id,
    sessionKey: pane.info.created_at,
    subscribePaneData: registry.subscribePaneData,
    syncPTYResize: true,
    disableMobileInput,
    reflowKey,
    redrawKey,
    reprintsOnResize: pane.info.reprints_on_resize,
    heldPtySize: pane.info.viewer_held ? { cols: pane.info.cols, rows: pane.info.rows } : null,
  });

  if (getSelectionRef) getSelectionRef.current = getSelection;

  const remount = useCallback(() => {
    mountedAtRef.current = Date.now();
    setEpoch((n) => n + 1);
  }, []);
  useEffect(() => {
    mountedAtRef.current = Date.now();
    return onDesktopNudge((skipIfScrolledUp) => {
      // A pane that was mounted a moment ago (the click that selected it) already is a fresh mount.
      if (Date.now() - mountedAtRef.current < NUDGE_MIN_GAP_MS) return;
      if (skipIfScrolledUp && isScrolledUp()) return;
      remount();
    });
  }, [isScrolledUp, remount, pane.info.id]);
  // Coming back to this window is an action too.
  useEffect(() => {
    const onFocus = () => nudgeDesktop(true);
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, []);

  // A phone holds this pane's size: using this window takes it back (only then, and throttled).
  const held = pane.info.viewer_held;
  const paneId = pane.info.id;
  const lastReclaimRef = useRef(0);
  const reclaimIfHeld = useCallback(() => {
    if (!held) return;
    const now = Date.now();
    if (now - lastReclaimRef.current < RECLAIM_THROTTLE_MS) return;
    lastReclaimRef.current = now;
    void tauri.reclaimPaneSize(paneId).then(() => {
      afterReclaim();
      nudgeDesktop(true);
    }, () => {});
  }, [held, paneId, afterReclaim]);
  useEffect(() => {
    if (!held) return;
    window.addEventListener('focus', reclaimIfHeld);
    window.addEventListener('keydown', reclaimIfHeld, true);
    return () => {
      window.removeEventListener('focus', reclaimIfHeld);
      window.removeEventListener('keydown', reclaimIfHeld, true);
    };
  }, [held, reclaimIfHeld]);

  return (
    <section
      className={chrome ? 'pm-terminal-stage' : 'pm-terminal-stage pm-terminal-stage--bare'}
      aria-label="Command terminal"
      onPointerDownCapture={held ? reclaimIfHeld : undefined}
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
        <div className="pm-terminal-viewer-held" role="status" onPointerDown={reclaimIfHeld}>
          <strong>Sized for your phone</strong>
          <span>Click here or switch to this window to use it on this PC.</span>
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
            title="Redraw from the PC screen"
            aria-label="Redraw from the PC screen"
            onClick={remount}
          >
            ↻
          </button>
        </div>
      )}
    </section>
  );
}
