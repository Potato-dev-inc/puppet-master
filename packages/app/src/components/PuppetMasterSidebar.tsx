// Desktop right-hand sidebar: the Puppet Master orchestrator (API chat or a CLI agent pane) plus the
// coordination/MCP-log tools tab. State and side effects live in ./sidebar/* hooks; this wires them to views.
import { useState } from 'react';
import type { McpLogEntry } from '@puppet-master/shared';
import type { BridgeClient } from '../lib/bridge';
import type { PaneRegistryApi } from '../hooks/usePaneRegistry';
import { AgentView } from './sidebar/AgentView';
import { BackendModelBar } from './sidebar/BackendModelBar';
import { SidebarHeader } from './sidebar/SidebarHeader';
import { SidebarTabs, type SidebarTab } from './sidebar/SidebarTabs';
import { ToolsView } from './sidebar/ToolsView';
import { useAttentionTracking } from './sidebar/useAttentionTracking';
import { useOrchestratorPane } from './sidebar/useOrchestratorPane';
import { useOrchestratorSession } from './sidebar/useOrchestratorSession';
import { useRemoteOrchestratorMessages } from './sidebar/useRemoteOrchestratorMessages';
import { useSidebarSettings } from './sidebar/useSidebarSettings';

interface Props {
  width: number;
  bridge: BridgeClient | null;
  bridgeReady: boolean;
  externalLogs: McpLogEntry[];
  registry: PaneRegistryApi;
  projectPath: string | null;
  onShowSettings: () => void;
  settingsRevision?: number;
}

export function PuppetMasterSidebar({
  width,
  bridge,
  bridgeReady,
  externalLogs,
  registry,
  projectPath,
  onShowSettings,
  settingsRevision = 0,
}: Props) {
  const [activeTab, setActiveTab] = useState<SidebarTab>('agent');
  const [mcpLogHeight, setMcpLogHeight] = useState(128);
  const { model, availableModels, backend, theme, changeBackend, changeModel, toggleTheme } =
    useSidebarSettings(settingsRevision);
  const session = useOrchestratorSession({ bridge, backend, model, externalLogs });
  const {
    cliBackend,
    orchestratorPane,
    orchestratorTunnel,
    orchestratorStarting,
    orchestratorError,
    mcpStatus,
    startOrchestratorPane,
  } = useOrchestratorPane(backend, registry, projectPath);

  useAttentionTracking({
    panes: registry.panes,
    busyRef: session.busyRef,
    logAutoApprove: session.logAutoApprove,
    runWake: session.runInternalWake,
  });
  useRemoteOrchestratorMessages(bridge, model, session);

  return (
    <aside
      className="flex-shrink-0 border-l border-pm-border bg-pm-panel flex flex-col"
      style={{ width }}
    >
      <SidebarHeader
        bridgeReady={bridgeReady}
        busy={session.busy}
        theme={theme}
        onNewChat={session.newChat}
        onToggleTheme={() => void toggleTheme()}
        onShowSettings={onShowSettings}
      />
      <BackendModelBar
        backend={backend}
        cliBackend={cliBackend}
        model={model}
        availableModels={availableModels}
        mcpStatus={mcpStatus}
        onBackendChange={(next) => void changeBackend(next)}
        onModelChange={(next) => void changeModel(next)}
      />
      <SidebarTabs active={activeTab} onChange={setActiveTab} />

      <div className="flex-1 min-h-0 flex flex-col">
        {activeTab === 'agent' ? (
          <AgentView
            cliBackend={cliBackend}
            projectPath={projectPath}
            orchestratorPane={orchestratorPane}
            orchestratorStarting={orchestratorStarting}
            orchestratorError={orchestratorError}
            orchestratorTunnel={orchestratorTunnel}
            onRetryOrchestrator={() => {
              if (cliBackend && projectPath) void startOrchestratorPane(cliBackend, projectPath);
            }}
            messages={session.messages}
            busy={session.busy}
            standbyPanes={session.standbyPanes}
            onInterrupt={session.interrupt}
          />
        ) : (
          <ToolsView
            bridge={bridge}
            bridgeReady={bridgeReady}
            logs={session.logs}
            logHeight={mcpLogHeight}
            onLogHeightChange={setMcpLogHeight}
          />
        )}
      </div>
    </aside>
  );
}
