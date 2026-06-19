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

const LS_SIDEBAR_WIDTH = 'pm-sidebar-width';
const MIN_SIDEBAR_WIDTH = 300;
const MAX_SIDEBAR_WIDTH = 800;

function clampSidebarWidth(width: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width));
}

function readSidebarWidth(): number {
  const stored = localStorage.getItem(LS_SIDEBAR_WIDTH);
  if (!stored) return 360;
  const parsed = Number.parseInt(stored, 10);
  return Number.isFinite(parsed) ? clampSidebarWidth(parsed) : 360;
}

export default function App() {
  const registry = usePaneRegistry();
  const { projectPath, setProjectPath } = useProjectPath();
  const bridgeApi = useBridge();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsRevision, setSettingsRevision] = useState(0);
  const [sidebarWidth, setSidebarWidth] = useState(readSidebarWidth);
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
        if (typeof patch.mobile_input_delay_ms === 'number') {
          merged.mobile_input_delay_ms = patch.mobile_input_delay_ms;
        }
        if (typeof patch.mobile_input_visible === 'boolean') {
          merged.mobile_input_visible = patch.mobile_input_visible;
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
              localStorage.setItem(LS_SIDEBAR_WIDTH, String(next));
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
      <SettingsPanel
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onSaved={bumpSettings}
      />
    </div>
  );
}
