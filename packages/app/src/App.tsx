import { useCallback, useEffect, useState } from 'react';
import type { AgentType, LlmProvider, OrchestratorBackend } from '@puppet-master/shared';
import { WorkspaceHeader } from './components/WorkspaceHeader';
import { TerminalGrid } from './components/TerminalGrid';
import { PuppetMasterSidebar } from './components/PuppetMasterSidebar';
import { SettingsPanel } from './components/SettingsPanel';
import { usePaneRegistry } from './hooks/usePaneRegistry';
import { useProjectPath } from './hooks/useProjectPath';
import { useBridge } from './hooks/useBridge';
import { loadSettings, saveSettings, syncPublicSettingsToBridge } from './lib/settings';
import { tauri } from './lib/tauri';

export default function App() {
  const registry = usePaneRegistry();
  const { projectPath, setProjectPath } = useProjectPath();
  const bridgeApi = useBridge();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsRevision, setSettingsRevision] = useState(0);
  const [sidebarWidth, setSidebarWidth] = useState(360);
  const bumpSettings = useCallback(() => setSettingsRevision((n) => n + 1), []);

  useEffect(() => {
    void syncPublicSettingsToBridge();
    let cancelled = false;
    let unlistenApply: (() => void) | null = null;

    void (async () => {
      unlistenApply = await tauri.onSettingsApply(async (patch) => {
        const current = await loadSettings();
        const merged = { ...current };
        if (patch.orchestrator_backend) {
          merged.orchestrator_backend = patch.orchestrator_backend as OrchestratorBackend;
        }
        if (patch.default_provider) {
          merged.default_provider = patch.default_provider as LlmProvider;
        }
        if (patch.default_model) {
          merged.default_model = patch.default_model;
        }
        await saveSettings(merged);
        bumpSettings();
      });
      if (cancelled) unlistenApply?.();
    })();

    return () => {
      cancelled = true;
      unlistenApply?.();
    };
  }, [bumpSettings]);

  const handleClose = async (paneId: string) => {
    await registry.killPane(paneId);
  };
  const handleKillAll = async () => {
    await registry.killAll();
  };
  const handleRestart = async () => {
    await registry.killAll();
  };
  const handleClear = () => {
    void registry.refresh();
  };

  const handleNewSession = async (agent: AgentType) => {
    await registry.spawnPane({ agent_type: agent, cwd: projectPath ?? undefined });
  };

  return (
    <div className="flex flex-col h-full">
      <WorkspaceHeader
        projectPath={projectPath}
        onProjectPathChange={setProjectPath}
        onKillAll={handleKillAll}
        onRestart={handleRestart}
        onClearBuffers={handleClear}
        onNewSession={handleNewSession}
      />
      <div className="flex flex-1 min-h-0">
        <TerminalGrid registry={registry} projectPath={projectPath} onClosePane={handleClose} />
        <div
          role="separator"
          aria-orientation="vertical"
          title="Resize sidebar"
          className="w-1 cursor-col-resize bg-pm-border/60 hover:bg-pm-accent transition-colors"
          onPointerDown={(event) => {
            event.preventDefault();
            const startX = event.clientX;
            const startWidth = sidebarWidth;
            const move = (moveEvent: PointerEvent) => {
              const delta = startX - moveEvent.clientX;
              setSidebarWidth(Math.min(560, Math.max(300, startWidth + delta)));
            };
            const up = () => {
              window.removeEventListener('pointermove', move);
              window.removeEventListener('pointerup', up);
            };
            window.addEventListener('pointermove', move);
            window.addEventListener('pointerup', up, { once: true });
          }}
        />
        <PuppetMasterSidebar
          width={sidebarWidth}
          bridge={bridgeApi.client}
          bridgeReady={bridgeApi.isReady}
          externalLogs={bridgeApi.externalLogs}
          registry={registry}
          projectPath={projectPath}
          onShowSettings={() => setSettingsOpen(true)}
          settingsRevision={settingsRevision}
        />
      </div>
      <SettingsPanel
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onSaved={bumpSettings}
      />
    </div>
  );
}
