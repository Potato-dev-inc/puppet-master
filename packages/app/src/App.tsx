import { useCallback, useEffect, useState } from 'react';
import type { AgentType } from '@puppet-master/shared';
import { WorkspaceHeader } from './components/WorkspaceHeader';
import { TerminalGrid } from './components/TerminalGrid';
import { PuppetMasterSidebar } from './components/PuppetMasterSidebar';
import { SettingsPanel } from './components/SettingsPanel';
import { HomeDashboard } from './components/HomeDashboard';
import { usePaneRegistry } from './hooks/usePaneRegistry';
import { useProjectPath } from './hooks/useProjectPath';
import { useBridge } from './hooks/useBridge';
import { clampSidebarWidth, loadSettings, saveSettings, syncPublicSettingsToBridge } from './lib/settings';
import {
  applyRemoteOrchestratorProviderPatch,
  publicSettingsToProviderPatch,
} from './lib/orchestrator-provider-sync';
import { tauri } from './lib/tauri';
import { LoadingScreen } from './components/LoadingScreen';
import { useBootGate } from './hooks/useBootGate';

type AppScreen = 'home' | 'workspace';

export default function App() {
  const registry = usePaneRegistry();
  const { projectPath, setProjectPath, ready: projectReady } = useProjectPath();
  const bridgeApi = useBridge();
  const [screen, setScreen] = useState<AppScreen>('home');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsRevision, setSettingsRevision] = useState(0);
  const [sidebarWidth, setSidebarWidth] = useState(360);
  const bumpSettings = useCallback(() => setSettingsRevision((n) => n + 1), []);

  useEffect(() => {
    void loadSettings().then((loaded) => setSidebarWidth(clampSidebarWidth(loaded.sidebar_width ?? 360)));
  }, [settingsRevision]);
  const boot = useBootGate({
    projectReady,
    bridgeReady: bridgeApi.isReady,
    registryReady: registry.initialReady,
  });

  useEffect(() => {
    void syncPublicSettingsToBridge();
    let cancelled = false;
    let unlistenApply: (() => void) | null = null;

    void (async () => {
      unlistenApply = await tauri.onSettingsApply(async (patch) => {
        const providerPatch = publicSettingsToProviderPatch(patch);
        if (providerPatch.backend || providerPatch.provider || providerPatch.model) {
          await applyRemoteOrchestratorProviderPatch(providerPatch);
        }

        if (typeof patch.mobile_input_delay_ms === 'number' || typeof patch.mobile_input_visible === 'boolean') {
          const current = await loadSettings();
          const merged = { ...current };
          if (typeof patch.mobile_input_delay_ms === 'number') {
            merged.mobile_input_delay_ms = patch.mobile_input_delay_ms;
          }
          if (typeof patch.mobile_input_visible === 'boolean') {
            merged.mobile_input_visible = patch.mobile_input_visible;
          }
          await saveSettings(merged);
        }

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
    <>
      {boot.showBoot && (
        <LoadingScreen steps={boot.steps} progress={boot.progress} exiting={boot.exiting} />
      )}

      {!boot.showBoot && screen === 'home' && (
        <HomeDashboard
          projectPath={projectPath}
          bridgeReady={bridgeApi.isReady}
          bridgeError={bridgeApi.error}
          registry={registry}
          settingsRevision={settingsRevision}
          onOpenWorkspace={() => setScreen('workspace')}
          onOpenSettings={() => setSettingsOpen(true)}
          onProjectPathChange={setProjectPath}
        />
      )}

      {!boot.showBoot && screen === 'workspace' && (
        <div className="flex flex-col h-full pm-workspace-enter">
          <WorkspaceHeader
            projectPath={projectPath}
            onProjectPathChange={setProjectPath}
            onKillAll={handleKillAll}
            onRestart={handleRestart}
            onClearBuffers={handleClear}
            onNewSession={handleNewSession}
            onGoHome={() => setScreen('home')}
          />
          <div className="flex flex-1 min-h-0">
            <TerminalGrid registry={registry} projectPath={projectPath} onClosePane={handleClose} />
            <div
              role="separator"
              aria-orientation="vertical"
              title="Resize sidebar"
              className="w-1 shrink-0 cursor-col-resize bg-pm-border/60 hover:bg-pm-accent transition-colors touch-none"
              onPointerDown={(event) => {
                event.preventDefault();
                const startX = event.clientX;
                const startWidth = sidebarWidth;
                const target = event.currentTarget;
                target.setPointerCapture(event.pointerId);

                const move = (moveEvent: globalThis.PointerEvent) => {
                  const delta = startX - moveEvent.clientX;
                  const next = clampSidebarWidth(startWidth + delta);
                  setSidebarWidth(next);
                  void loadSettings().then((current) => saveSettings({ ...current, sidebar_width: next }));
                };
                const up = (upEvent: globalThis.PointerEvent) => {
                  target.releasePointerCapture(upEvent.pointerId);
                  target.removeEventListener('pointermove', move);
                  target.removeEventListener('pointerup', up);
                  target.removeEventListener('pointercancel', up);
                };
                target.addEventListener('pointermove', move);
                target.addEventListener('pointerup', up);
                target.addEventListener('pointercancel', up);
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
        </div>
      )}

      <SettingsPanel
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onSaved={bumpSettings}
        projectPath={projectPath}
        onProjectPathChange={setProjectPath}
        onSidebarWidthChange={setSidebarWidth}
      />
    </>
  );
}
