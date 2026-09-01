import { useEffect, useMemo, useRef, useState } from 'react';
import { getDefaultTerminalAgentType, type AgentType } from '@puppet-master/shared';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { usePaneRegistry } from './hooks/usePaneRegistry';
import {
  detachedPaneTitle,
  detachedWindowSizeFromGrid,
  openDetachedPaneWindow,
} from './lib/detached-pane-window';
import { tauri, type WorkerLaunchConfig } from './lib/tauri';

// Survives React StrictMode remounts — worker host must spawn at most once per process.
let workerHostLaunchStarted = false;

async function launchDetachedWorker(
  config: WorkerLaunchConfig,
  spawnPane: ReturnType<typeof usePaneRegistry>['spawnPane'],
  defaultAgent: AgentType,
): Promise<void> {
  if (config.cwd) {
    await tauri.setProjectPath(config.cwd);
  }

  const agentType = (config.agent_type || defaultAgent) as AgentType;

  // ponytail: worker relaunch must not reuse a dead PTY or orphan opencode serve.
  const existing = await tauri.listPanes();
  for (const pane of existing.filter((entry) => entry.agent_type === agentType)) {
    if (config.pane_id && pane.id === config.pane_id && pane.status !== 'error' && !config.force_new) {
      continue;
    }
    await tauri.killPane(pane.id).catch(() => {});
  }

  let paneId = config.pane_id ?? null;
  if (paneId && !config.force_new) {
    const live = (await tauri.listPanes()).find(
      (pane) => pane.id === paneId && pane.status !== 'error',
    );
    if (!live) {
      paneId = null;
    }
  } else {
    paneId = null;
  }

  if (!paneId) {
    paneId = await spawnPane({
      agent_type: agentType,
      cwd: config.cwd ?? undefined,
      cols: config.cols,
      rows: config.rows,
      pane_id: config.pane_id ?? undefined,
    });
  }

  const pane = (await tauri.listPanes()).find((entry) => entry.id === paneId);
  const cols = pane?.cols ?? config.cols;
  const rows = pane?.rows ?? config.rows;

  await openDetachedPaneWindow(
    paneId,
    detachedPaneTitle(agentType, paneId),
    detachedWindowSizeFromGrid(cols, rows),
    { workerHost: true },
  );
}

/**
 * Hidden host for `puppet-master worker`: spawns a pane and pops it out with the
 * same detached TerminalApp window used by the main grid's ↗ control.
 */
export default function WorkerHostBootstrap() {
  const { spawnPane, initialReady } = usePaneRegistry();
  const defaultAgent = useMemo(() => getDefaultTerminalAgentType(), []);
  const [error, setError] = useState<string | null>(null);
  const spawnPaneRef = useRef(spawnPane);
  spawnPaneRef.current = spawnPane;

  useEffect(() => {
    if (!error) return;
    void getCurrentWebviewWindow().show().catch(() => {});
  }, [error]);

  useEffect(() => {
    void getCurrentWebviewWindow().hide().catch(() => {});
  }, []);

  useEffect(() => {
    if (!initialReady || workerHostLaunchStarted) return;
    workerHostLaunchStarted = true;

    let cancelled = false;
    void (async () => {
      try {
        const config = await tauri.getWorkerLaunch();
        if (cancelled) return;
        if (!config) {
          setError('Worker host requires puppet-master worker launch flags or env.');
          return;
        }
        await launchDetachedWorker(config, spawnPaneRef.current, defaultAgent);
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [defaultAgent, initialReady]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void tauri.onPaneReattach(() => {
      void tauri.exitApp();
    }).then((next) => {
      if (disposed) next();
      else unlisten = next;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!error) return <div hidden />;

  return (
    <div className="pm-terminal-app">
      <main className="pm-terminal-shell">
        <div className="pm-terminal-error pm-terminal-empty">{error}</div>
      </main>
    </div>
  );
}
