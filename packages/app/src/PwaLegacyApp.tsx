// LEGACY mobile PWA (backup, kept for a possible return): the orchestrator-centric shell with the
// Orchestrator chat / Panes tabs (components/pwa/*). Open it with `?pwa=legacy`; the default mobile
// UI is PwaApp.tsx (agent tabs, no orchestrator control).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AgentType, OrchestratorChatEvent, PaneInfo } from '@puppet-master/shared';
import { useBridgePaneRegistry } from './hooks/useBridgePaneRegistry';
import { useOrchestratorViewportReporter } from './hooks/useOrchestratorViewportReporter';
import { usePaneTunnel } from './hooks/usePaneTunnel';
import { useVisualViewportSync } from './hooks/useVisualViewport';
import { makeBridgeClient, subscribeBridgeEvents, type BridgeClient } from './lib/bridge';
import { routeBridgeEventToPaneTunnel } from './lib/bridge-pane-tunnel';
import { DEFAULT_PUBLIC_SETTINGS, type PublicSettings } from './lib/bridge-settings';
import { ngrokRequestHeaders } from './lib/bridge-ngrok';
import { mergeBridgeHeaders } from './lib/mobile-pairing-auth';
import { LS_BRIDGE_URL, resolveBridgeBaseUrl } from './lib/bridge-url';
import {
  findOrchestratorPane,
  isCliOrchestratorBackend,
  isOrchestratorPaneId,
} from './lib/orchestrator-panes';
import {
  PairInviteScreen,
  readPairingInviteCode,
  clearPairingInviteFromUrl,
} from './components/PairInviteScreen';
import { FloatingMobileMenu } from './components/pwa/FloatingMobileMenu';
import { OrchestratorTab } from './components/pwa/OrchestratorTab';
import { PanesTab } from './components/pwa/PanesTab';
import { SetupScreen } from './components/pwa/SetupScreen';

async function syncBridgePanes(
  client: BridgeClient,
  apply: (panes: PaneInfo[]) => void,
): Promise<void> {
  const list = await client.listPanes();
  apply(list);
}

export default function PwaLegacyApp() {
  const [inviteCode, setInviteCode] = useState(() => readPairingInviteCode());
  const [bridgeUrl, setBridgeUrl] = useState<string | null>(() =>
    readPairingInviteCode()
      ? null
      : resolveBridgeBaseUrl(localStorage.getItem(LS_BRIDGE_URL), window.location),
  );
  const [bridge, setBridge] = useState<BridgeClient | null>(null);
  const [bridgeReady, setBridgeReady] = useState(false);
  const [chatEvents, setChatEvents] = useState<OrchestratorChatEvent[]>([]);
  const [panes, setPanes] = useState<PaneInfo[]>([]);
  const [settings, setSettings] = useState<PublicSettings>(DEFAULT_PUBLIC_SETTINGS);
  const [settingsHydrated, setSettingsHydrated] = useState(false);
  const [tab, setTab] = useState<'chat' | 'panes'>('chat');
  const [editingUrl, setEditingUrl] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [selectedPaneId, setSelectedPaneId] = useState<string | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [spawnError, setSpawnError] = useState<string | null>(null);
  const registry = useBridgePaneRegistry(bridge);
  const registryRef = useRef(registry);
  registryRef.current = registry;

  /** Single entry point for a fresh pane list: React state plus the terminal registry. */
  const applyPanes = useCallback((list: PaneInfo[]) => {
    setPanes(list);
    registryRef.current.setPanesFromList(list);
  }, []);

  const orchestratorBackend = settings.orchestrator_backend ?? 'api';
  const cliOrchestratorBackend = isCliOrchestratorBackend(orchestratorBackend)
    ? orchestratorBackend
    : null;
  const orchestratorPaneId = useMemo(() => {
    if (!cliOrchestratorBackend) return null;
    const info = findOrchestratorPane(panes, cliOrchestratorBackend);
    return info?.id ?? null;
  }, [cliOrchestratorBackend, panes]);

  const orchestratorReady = useMemo(() => {
    if (!cliOrchestratorBackend) return true;
    return !!findOrchestratorPane(panes, cliOrchestratorBackend);
  }, [cliOrchestratorBackend, panes]);

  const mobileOrchestratorTunnel = usePaneTunnel(bridge, orchestratorPaneId, 'mobile');
  const orchestratorTunnelRef = useRef(mobileOrchestratorTunnel);
  orchestratorTunnelRef.current = mobileOrchestratorTunnel;

  useVisualViewportSync(!!bridge);
  useOrchestratorViewportReporter(bridge, tab === 'chat' && !!cliOrchestratorBackend);

  useEffect(() => {
    if (!bridgeUrl) return;
    const c = makeBridgeClient(bridgeUrl);
    setBridge(c);
    setBridgeReady(false);
    setSettingsHydrated(false);

    fetch(`${bridgeUrl}/health`, {
      signal: AbortSignal.timeout(3000),
      headers: mergeBridgeHeaders(ngrokRequestHeaders(bridgeUrl)),
    })
      .then((r) => { if (r.ok) setBridgeReady(true); })
      .catch(() => {});

    void c
      .getSettings()
      .then((next) => {
        setSettings(next);
        setSettingsHydrated(true);
        return syncBridgePanes(c, applyPanes);
      })
      .catch(() => {
        setSettingsHydrated(true);
      });

    const unsub = subscribeBridgeEvents(
      bridgeUrl,
      (ev) => {
        if (ev.type === 'chat') {
          setChatEvents((prev) => [...prev, ev.event].slice(-500));
        }
        if (ev.type === 'panes') {
          applyPanes(ev.panes);
        }
        if (ev.type === 'terminal') {
          if (isOrchestratorPaneId(ev.pane_id)) {
            routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
          } else {
            registryRef.current.ingestTerminalData(ev.pane_id, ev.data, ev.offset);
          }
        }
        if (ev.type === 'pane-status') {
          registryRef.current.updatePaneStatus(ev.pane_id, ev.status);
        }
        if (ev.type === 'pane-resize') {
          if (isOrchestratorPaneId(ev.pane_id)) {
            routeBridgeEventToPaneTunnel(ev, orchestratorTunnelRef.current);
          } else {
            registryRef.current.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows);
          }
        }
        if (ev.type === 'settings') {
          setSettings(ev.settings);
        }
      },
    );

    return unsub;
  }, [bridgeUrl, applyPanes]);

  useEffect(() => {
    if (tab !== 'panes') return;
    if (selectedPaneId && panes.some((pane) => pane.id === selectedPaneId)) return;
    setSelectedPaneId(panes[0]?.id ?? null);
  }, [panes, selectedPaneId, tab]);

  const patchSettings = useCallback(async (patch: Partial<PublicSettings>) => {
    if (!bridge) return;
    const updated = await bridge.patchSettings(patch);
    setSettings(updated);
  }, [bridge]);

  const refreshPanes = useCallback(async () => {
    if (!bridge) return;
    applyPanes(await bridge.listPanes());
  }, [bridge, applyPanes]);

  const spawnPane = useCallback(async (agentType: AgentType) => {
    if (!bridge) return;
    setSpawning(true);
    setSpawnError(null);
    try {
      const { pane_id } = await bridge.spawnPane({ agent_type: agentType, cols: 120, rows: 30 });
      await refreshPanes();
      setSelectedPaneId(pane_id);
      setTab('panes');
      setMenuOpen(false);
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    } finally {
      setSpawning(false);
    }
  }, [bridge, refreshPanes]);

  const killSelectedPane = useCallback(async () => {
    if (!bridge || !selectedPaneId) return;
    setSpawnError(null);
    try {
      await bridge.killPane(selectedPaneId);
      setSelectedPaneId(null);
      await refreshPanes();
    } catch (e) {
      setSpawnError(e instanceof Error ? e.message : String(e));
    }
  }, [bridge, refreshPanes, selectedPaneId]);

  useEffect(() => {
    if (!bridge || !bridgeReady || !settingsHydrated || !cliOrchestratorBackend || orchestratorReady) {
      return;
    }

    let cancelled = false;
    const tick = () => {
      void syncBridgePanes(bridge, (list) => {
        if (!cancelled) applyPanes(list);
      }).catch(() => {});
    };

    tick();
    const id = window.setInterval(tick, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [bridge, bridgeReady, settingsHydrated, cliOrchestratorBackend, orchestratorReady, applyPanes]);

  const handleInviteConnected = useCallback((url: string) => {
    clearPairingInviteFromUrl();
    setInviteCode(null);
    setBridgeUrl(url);
    setEditingUrl(false);
  }, []);

  const handleInviteGiveUp = useCallback(() => {
    clearPairingInviteFromUrl();
    setInviteCode(null);
  }, []);

  if (inviteCode) {
    return (
      <div className="pwa-shell bg-pm-bg text-zinc-100">
        <PairInviteScreen
          pairingCode={inviteCode}
          onConnected={handleInviteConnected}
          onGiveUp={handleInviteGiveUp}
        />
      </div>
    );
  }

  if (!bridgeUrl || editingUrl) {
    return (
      <div className="pwa-shell bg-pm-bg text-zinc-100">
        <SetupScreen
          onConnect={(url) => {
            setBridgeUrl(url);
            setEditingUrl(false);
          }}
        />
      </div>
    );
  }

  return (
    <div className="pwa-shell bg-pm-bg text-zinc-100">
      <main className="pwa-main" aria-label="Puppet Master mobile workspace">
        {bridge && tab === 'chat' && (
          <section className="pwa-stage" aria-label="Orchestrator">
            <OrchestratorTab
              bridge={bridge}
              chatEvents={chatEvents}
              bridgeReady={bridgeReady}
              settings={settings}
              registry={registry}
              orchestratorTunnel={mobileOrchestratorTunnel}
              settingsHydrated={settingsHydrated}
            />
          </section>
        )}
        {bridge && tab === 'panes' && (
          <section className="pwa-stage" aria-label="Panes">
            <PanesTab
              panes={panes}
              bridgeReady={bridgeReady}
              registry={registry}
              mobileInputDelayMs={settings.mobile_input_delay_ms}
              mobileInputVisible={settings.mobile_input_visible}
              selected={selectedPaneId}
              onSelect={setSelectedPaneId}
            />
          </section>
        )}
        {!bridge && (
          <div className="flex items-center justify-center h-full text-xs text-pm-muted">
            Connecting…
          </div>
        )}
      </main>
      <FloatingMobileMenu
        open={menuOpen}
        onOpenChange={setMenuOpen}
        bridgeReady={bridgeReady}
        tab={tab}
        onTabChange={(next) => {
          setTab(next);
          setMenuOpen(false);
        }}
        onEditConnection={() => {
          setEditingUrl(true);
          setMenuOpen(false);
        }}
        settings={settings}
        onPatchSettings={patchSettings}
        panes={panes}
        selectedPaneId={selectedPaneId}
        onSelectPane={(paneId) => {
          setSelectedPaneId(paneId);
          setMenuOpen(false);
        }}
        onRefreshPanes={refreshPanes}
        onSpawnPane={spawnPane}
        onKillSelectedPane={killSelectedPane}
        spawning={spawning}
        spawnError={spawnError}
      />
    </div>
  );
}
