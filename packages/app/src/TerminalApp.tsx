import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  getDefaultTerminalAgentType,
  listLaunchPresets,
  type AgentType,
} from '@puppet-master/shared';
import { useBridge } from './hooks/useBridge';
import { usePaneRegistry, type PaneData } from './hooks/usePaneRegistry';
import { useProjectPath } from './hooks/useProjectPath';
import { useTerminalSession } from './hooks/useTerminalSession';
import { tauri } from './lib/tauri';
import { detachedWindowSizeFromGrid, openDetachedPaneWindow } from './lib/detached-pane-window';

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-pm-ok',
  waiting_input: 'bg-pm-warn',
  idle: 'bg-pm-muted',
  error: 'bg-pm-err',
};

function paneTitle(agentType: string): string {
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
    default:
      return agentType;
  }
}

function apiSnippet(baseUrl: string, paneId: string | null, draft: string, appendNewline: boolean): string {
  const safePaneId = paneId ?? '<pane-id>';
  return [
    `curl -X POST "${baseUrl}/panes/${safePaneId}/input" \\`,
    '  -H "Content-Type: application/json" \\',
    `  -d '${JSON.stringify({ text: draft || 'dir', append_newline: appendNewline })}'`,
    '',
    `curl "${baseUrl}/panes/${safePaneId}/buffer?lines=80"`,
    `curl "${baseUrl}/panes/${safePaneId}/snapshot"`,
  ].join('\n');
}

function TerminalViewport({
  pane,
  registry,
  chrome = true,
  disableMobileInput = false,
}: {
  pane: PaneData;
  registry: ReturnType<typeof usePaneRegistry>;
  chrome?: boolean;
  disableMobileInput?: boolean;
}) {
  const { containerRef } = useTerminalSession({
    paneId: pane.info.id,
    sessionKey: pane.info.created_at,
    subscribePaneData: registry.subscribePaneData,
    syncPTYResize: true,
    disableMobileInput,
  });

  return (
    <section
      className={chrome ? 'pm-terminal-stage' : 'pm-terminal-stage pm-terminal-stage--bare'}
      aria-label="Command terminal"
    >
      {chrome && (
        <div className="pm-terminal-titlebar">
          <span className={`pm-terminal-status ${STATUS_COLOR[pane.status] ?? 'bg-pm-muted'}`} />
          <strong>{paneTitle(pane.info.agent_type)}</strong>
          <span>pid {pane.info.pid}</span>
          <span>{pane.info.cols}x{pane.info.rows}</span>
        </div>
      )}
      <div ref={containerRef} className="terminal-host pm-terminal-host" />
    </section>
  );
}

export default function TerminalApp() {
  const params = useMemo(() => new URLSearchParams(window.location.search), []);
  const lockedPaneId = params.get('pane');
  const detached = params.has('detached');
  const registry = usePaneRegistry();
  const bridge = useBridge();
  const { projectPath } = useProjectPath();
  const defaultTerminalType = useMemo(() => getDefaultTerminalAgentType(), []);
  const [selectedPaneId, setSelectedPaneId] = useState<string | null>(lockedPaneId);
  const [agentType, setAgentType] = useState<AgentType>(defaultTerminalType);
  const [draft, setDraft] = useState('dir');
  const [appendNewline, setAppendNewline] = useState(true);
  const [buffer, setBuffer] = useState('');
  const [snapshot, setSnapshot] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [detachedLayoutReady, setDetachedLayoutReady] = useState(!detached);

  const panes = registry.paneList;
  const selectedPane = selectedPaneId ? registry.panes.get(selectedPaneId) ?? null : null;
  const selectedPaneCols = selectedPane?.info.cols;
  const selectedPaneRows = selectedPane?.info.rows;
  const apiBaseUrl = bridge.client?.baseUrl ?? 'http://127.0.0.1:17321';
  const presets = useMemo(() => listLaunchPresets(), []);

  const selectOrSpawnTerminal = useCallback(async () => {
    if (lockedPaneId || !registry.initialReady || selectedPaneId) return;
    const existingTerminal = registry.paneList.find((pane) => pane.info.agent_type === defaultTerminalType);
    if (existingTerminal) {
      setSelectedPaneId(existingTerminal.info.id);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const paneId = await registry.spawnPane({
        agent_type: defaultTerminalType,
        cwd: projectPath ?? undefined,
        cols: 120,
        rows: 32,
      });
      setSelectedPaneId(paneId);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [defaultTerminalType, lockedPaneId, projectPath, registry, selectedPaneId]);

  useEffect(() => {
    void selectOrSpawnTerminal();
  }, [selectOrSpawnTerminal]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void tauri.onPaneDetach((event) => {
      void openDetachedPaneWindow(
        event.pane_id,
        event.title ?? `Pane ${event.pane_id.slice(0, 8)}`,
        detachedWindowSizeFromGrid(event.cols, event.rows),
      );
    }).then((next) => {
      if (disposed) {
        next();
      } else {
        unlisten = next;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (lockedPaneId) return;
    if (selectedPaneId && registry.panes.has(selectedPaneId)) return;
    setSelectedPaneId(panes[0]?.info.id ?? null);
  }, [lockedPaneId, panes, registry.panes, selectedPaneId]);

  useEffect(() => {
    if (!detached || !selectedPaneId) return;
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void tauri.onCurrentWindowCloseRequested(() => {
      void tauri.emitPaneReattach(selectedPaneId);
    }).then((next) => {
      if (disposed) {
        next();
      } else {
        unlisten = next;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [detached, selectedPaneId]);

  useEffect(() => {
    if (!detached) {
      setDetachedLayoutReady(true);
      return;
    }
    if (!selectedPaneId || selectedPaneCols === undefined || selectedPaneRows === undefined) {
      setDetachedLayoutReady(false);
      return;
    }

    let cancelled = false;
    setDetachedLayoutReady(false);
    void (async () => {
      const size = detachedWindowSizeFromGrid(selectedPaneCols, selectedPaneRows);
      try {
        await tauri.resizeCurrentWindow(size.width, size.height);
      } catch (err) {
        console.warn('[TerminalApp] failed to pre-size detached terminal', err);
      }
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      if (!cancelled) {
        setDetachedLayoutReady(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [detached, selectedPaneCols, selectedPaneId, selectedPaneRows]);

  const spawnPane = async () => {
    setBusy(true);
    setError(null);
    try {
      const paneId = await registry.spawnPane({
        agent_type: agentType,
        cwd: projectPath ?? undefined,
        cols: 120,
        rows: 32,
      });
      setSelectedPaneId(paneId);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const sendInput = async () => {
    if (!selectedPaneId || !draft) return;
    setBusy(true);
    setError(null);
    try {
      if (bridge.client) {
        await bridge.client.writeInput(selectedPaneId, draft, appendNewline);
      } else {
        await registry.writeInput(selectedPaneId, draft, appendNewline);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const readBuffer = async () => {
    if (!selectedPaneId) return;
    setBusy(true);
    setError(null);
    try {
      const content = bridge.client
        ? await bridge.client.readBuffer(selectedPaneId, 120)
        : await tauri.readBuffer(selectedPaneId, 120);
      setBuffer(content);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const readSnapshot = async () => {
    if (!selectedPaneId) return;
    setBusy(true);
    setError(null);
    try {
      const content = bridge.client
        ? await bridge.client.readSnapshot(selectedPaneId)
        : await tauri.readSnapshot(selectedPaneId);
      setSnapshot(content);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const killSelected = async () => {
    if (!selectedPaneId) return;
    setBusy(true);
    setError(null);
    try {
      await registry.killPane(selectedPaneId);
      setSelectedPaneId(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const reattachSelected = async () => {
    if (!selectedPaneId) return;
    setError(null);
    try {
      await tauri.emitPaneReattach(selectedPaneId);
      await tauri.closeCurrentWindow();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="pm-terminal-app">
      {detached && selectedPane && (
        <button
          type="button"
          className="pm-terminal-reattach-button"
          title="Move pane back to app"
          aria-label="Move pane back to app"
          onClick={() => void reattachSelected()}
        >
          ↙
        </button>
      )}
      <main className={detached ? 'pm-terminal-shell pm-terminal-shell--detached' : 'pm-terminal-shell'}>
        <section className="pm-terminal-main">
          {selectedPane && detached ? (
            detachedLayoutReady ? (
              <TerminalViewport
                pane={selectedPane}
                registry={registry}
                chrome={false}
                disableMobileInput
              />
            ) : (
              <div className="pm-terminal-detached-preflight" aria-hidden />
            )
          ) : selectedPane ? (
            <TerminalViewport pane={selectedPane} registry={registry} />
          ) : (
            <div className="pm-terminal-empty">
              {busy ? 'Opening command prompt...' : 'No terminal pane selected.'}
            </div>
          )}
        </section>

        {!detached && (
        <aside className="pm-terminal-dashboard" aria-label="Terminal API dashboard">
          <div className="pm-terminal-dashboard-head">
            <div>
              <p>PTY Terminal</p>
              <h1>Command Console</h1>
            </div>
            <span className={bridge.isReady ? 'pm-terminal-pill ok' : 'pm-terminal-pill'}>
              {bridge.isReady ? 'API online' : 'API scanning'}
            </span>
          </div>

          {error && <div className="pm-terminal-error">{error}</div>}

          <div className="pm-terminal-control">
            <label>
              Pane
              <select
                value={selectedPaneId ?? ''}
                onChange={(event) => setSelectedPaneId(event.target.value || null)}
              >
                <option value="">Select a pane</option>
                {panes.map((pane) => (
                  <option key={pane.info.id} value={pane.info.id}>
                    {pane.info.agent_type} - {pane.status} - {pane.info.id.slice(0, 8)}
                  </option>
                ))}
              </select>
            </label>

            <div className="pm-terminal-row">
              <select
                value={agentType}
                onChange={(event) => setAgentType(event.target.value as AgentType)}
              >
                {presets.map((preset) => (
                  <option key={preset.type} value={preset.type}>
                    {preset.label}
                  </option>
                ))}
              </select>
              <button type="button" onClick={() => void spawnPane()} disabled={busy}>
                New
              </button>
              <button type="button" onClick={() => void killSelected()} disabled={busy || !selectedPaneId}>
                Kill
              </button>
            </div>
          </div>

          <div className="pm-terminal-control">
            <label>
              Input
              <textarea
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
                    event.preventDefault();
                    void sendInput();
                  }
                }}
              />
            </label>
            <label className="pm-terminal-check">
              <input
                type="checkbox"
                checked={appendNewline}
                onChange={(event) => setAppendNewline(event.target.checked)}
              />
              Send Enter after input
            </label>
            <div className="pm-terminal-row">
              <button type="button" onClick={() => void sendInput()} disabled={busy || !selectedPaneId || !draft}>
                Send Input
              </button>
              <button type="button" onClick={() => void readBuffer()} disabled={busy || !selectedPaneId}>
                Read Buffer
              </button>
              <button type="button" onClick={() => void readSnapshot()} disabled={busy || !selectedPaneId}>
                Snapshot
              </button>
            </div>
          </div>

          <div className="pm-terminal-api">
            <div className="pm-terminal-api-label">HTTP API</div>
            <code>{apiBaseUrl}</code>
            <pre>{apiSnippet(apiBaseUrl, selectedPaneId, draft, appendNewline)}</pre>
          </div>

          {(buffer || snapshot) && (
            <div className="pm-terminal-output">
              <div className="pm-terminal-api-label">{snapshot ? 'Snapshot' : 'Buffer'}</div>
              <pre>{snapshot || buffer}</pre>
            </div>
          )}
        </aside>
        )}
      </main>
    </div>
  );
}
