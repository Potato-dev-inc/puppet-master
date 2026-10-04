// Lifecycle of the CLI-orchestrator pane (Claude/Codex/OpenCode/... as the sidebar brain):
// find it in the registry, (re)start it for the current project, and react to the "ensure" event.
import { useCallback, useEffect, useMemo, useState } from 'react';
import type { OrchestratorBackend } from '@puppet-master/shared';
import type { PaneRegistryApi } from '../../hooks/usePaneRegistry';
import { usePaneTunnel } from '../../hooks/usePaneTunnel';
import {
  ensureOrchestratorPane,
  findOrchestratorPane,
  isCliOrchestratorBackend,
  type CliOrchestratorBackend,
} from '../../lib/orchestrator-panes';
import { isValidProjectPath, projectPathsEqual } from '../../lib/project-path';
import { tauri } from '../../lib/tauri';

export function useOrchestratorPane(
  backend: OrchestratorBackend,
  registry: PaneRegistryApi,
  projectPath: string | null,
) {
  const [orchestratorStarting, setOrchestratorStarting] = useState(false);
  const [orchestratorError, setOrchestratorError] = useState<string | null>(null);
  const [mcpStatus, setMcpStatus] = useState<string | null>(null);

  const cliBackend = isCliOrchestratorBackend(backend) ? backend : null;
  const orchestratorPane = useMemo(() => {
    if (!cliBackend) return undefined;
    const info = findOrchestratorPane(
      registry.paneList.map((pane) => pane.info),
      cliBackend,
    );
    if (!info) return undefined;
    return registry.panes.get(info.id);
  }, [cliBackend, registry.paneList, registry.panes]);

  const orchestratorTunnel = usePaneTunnel(null, orchestratorPane?.info.id, 'desktop', registry);

  const startOrchestratorPane = useCallback(async (activeBackend: CliOrchestratorBackend, cwd: string) => {
    setOrchestratorStarting(true);
    setOrchestratorError(null);
    setMcpStatus(null);
    try {
      await ensureOrchestratorPane(activeBackend, cwd);
      setMcpStatus('Puppet Master MCP configured');
      await registry.refresh();
    } catch (err) {
      setOrchestratorError(err instanceof Error ? err.message : String(err));
    } finally {
      setOrchestratorStarting(false);
    }
  }, [registry]);

  const orchestratorPaneId = orchestratorPane?.info.id;
  const orchestratorPaneCwd = orchestratorPane?.info.cwd;
  const orchestratorPaneStatus = orchestratorPane?.status;

  // Start the pane when missing or pointed at another project. The pane's status is a dependency so
  // the check re-runs when it changes (an errored pane is deliberately left alone, no auto-restart).
  useEffect(() => {
    if (!cliBackend || !isValidProjectPath(projectPath)) {
      setOrchestratorError(null);
      setOrchestratorStarting(false);
      return;
    }
    if (orchestratorStarting) return;
    if (!orchestratorPaneId) {
      void startOrchestratorPane(cliBackend, projectPath);
      return;
    }
    if (!orchestratorPaneCwd || !projectPathsEqual(orchestratorPaneCwd, projectPath)) {
      void startOrchestratorPane(cliBackend, projectPath);
    }
  }, [
    cliBackend,
    projectPath,
    orchestratorPaneId,
    orchestratorPaneCwd,
    orchestratorPaneStatus,
    orchestratorStarting,
    startOrchestratorPane,
  ]);

  // The backend (e.g. an MCP call) can ask the desktop to ensure an orchestrator pane exists.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await tauri.onOrchestratorEnsure(async (payload) => {
        if (!isCliOrchestratorBackend(payload.backend as OrchestratorBackend)) return;
        const cwd = projectPath ?? (await tauri.getProjectPath());
        if (isValidProjectPath(cwd)) void startOrchestratorPane(payload.backend as CliOrchestratorBackend, cwd);
      });
      if (cancelled) unlisten?.();
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [projectPath, startOrchestratorPane]);

  return {
    cliBackend,
    orchestratorPane,
    orchestratorTunnel,
    orchestratorStarting,
    orchestratorError,
    mcpStatus,
    startOrchestratorPane,
  };
}
