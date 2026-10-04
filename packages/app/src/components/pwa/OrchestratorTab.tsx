// "Orchestrator" tab of the mobile PWA: the CLI orchestrator terminal, or the API-backend chat.
import { useMemo } from 'react';
import type { OrchestratorBackend, OrchestratorChatEvent } from '@puppet-master/shared';
import type { useBridgePaneRegistry } from '../../hooks/useBridgePaneRegistry';
import type { PaneTunnelApi } from '../../hooks/usePaneTunnel';
import type { BridgeClient } from '../../lib/bridge';
import type { PublicSettings } from '../../lib/bridge-settings';
import { findOrchestratorPane, isCliOrchestratorBackend } from '../../lib/orchestrator-panes';
import { OrchestratorTerminal } from '../OrchestratorTerminal';
import { ChatTab } from './ChatTab';

export function OrchestratorTab({
  bridge,
  bridgeReady,
  chatEvents,
  settings,
  registry,
  orchestratorTunnel,
  settingsHydrated,
}: {
  bridge: BridgeClient;
  bridgeReady: boolean;
  settingsHydrated: boolean;
  chatEvents: OrchestratorChatEvent[];
  settings: PublicSettings;
  registry: ReturnType<typeof useBridgePaneRegistry>;
  orchestratorTunnel: PaneTunnelApi;
}) {
  const backend = settings.orchestrator_backend ?? 'api';
  const cliBackend = isCliOrchestratorBackend(backend) ? backend : null;

  const orchestratorPane = useMemo(() => {
    if (!cliBackend) return undefined;
    const info = findOrchestratorPane(registry.paneList.map((p) => p.info), cliBackend);
    if (!info) return undefined;
    return registry.panes.get(info.id);
  }, [cliBackend, registry.paneList, registry.panes]);

  const orchestratorPaneView = useMemo(() => {
    if (!orchestratorPane) return undefined;
    const info = orchestratorTunnel.mergePaneInfo(orchestratorPane.info) ?? orchestratorPane.info;
    return { ...orchestratorPane, info };
  }, [orchestratorPane, orchestratorTunnel.mergePaneInfo]);

  const patchBackend = async (next: OrchestratorBackend) => {
    await bridge.patchSettings({ orchestrator_backend: next });
  };

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex-1 min-h-0 flex flex-col">
        {cliBackend ? (
          <OrchestratorTerminal
            backend={cliBackend}
            pane={orchestratorPaneView}
            starting={!orchestratorPane && bridgeReady && settingsHydrated}
            error={null}
            subscribePaneData={orchestratorTunnel.subscribePaneData}
            onRetry={() => void patchBackend(cliBackend)}
            transport={orchestratorTunnel.transport}
            syncPTYResize={false}
            mobileInputDelayMs={settings.mobile_input_delay_ms}
            mobileInputVisible={settings.mobile_input_visible}
          />
        ) : (
          <ChatTab bridge={bridge} chatEvents={chatEvents} bridgeReady={bridgeReady} />
        )}
      </div>
    </div>
  );
}
