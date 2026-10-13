// Mobile PWA root: watch and reply to the agents running on the PC, one tab per agent pane (like
// terminal tabs) plus an "All" overview. It does not drive the orchestrator. The previous
// orchestrator-centric mobile UI is kept as PwaLegacyApp.tsx (open with `?pwa=legacy`).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AgentType, PaneInfo } from '@puppet-master/shared';
import { useBridgePaneRegistry } from './hooks/useBridgePaneRegistry';
import { usePaneViewportLease } from './hooks/usePaneViewportLease';
import { useVisualViewportSync } from './hooks/useVisualViewport';
import { makeBridgeClient, type BridgeClient } from './lib/bridge';
import { subscribeBridgeEventsHandle, type BridgeEventsHandle } from './lib/bridge-sse';
import { DEFAULT_PUBLIC_SETTINGS, type PublicSettings } from './lib/bridge-settings';
import { ngrokRequestHeaders } from './lib/bridge-ngrok';
import { mergeBridgeHeaders } from './lib/mobile-pairing-auth';
import { LS_BRIDGE_URL, resolveBridgeBaseUrl } from './lib/bridge-url';
import { listingFromBody, type LaunchEntry } from './lib/agentRegistry';
import { ALL_TAB, orderPanes, phoneLaunchEntries, tabLabels } from './lib/mobile-agent-tabs';
import {
  PairInviteScreen,
  readPairingInviteCode,
  clearPairingInviteFromUrl,
} from './components/PairInviteScreen';
import { BridgePaneTerminal, useBridgePaneTransport } from './components/BridgePaneTerminal';
import { AgentTabBar } from './components/mobile/AgentTabBar';
import { AgentsOverview } from './components/mobile/AgentsOverview';
import { MenuSheet, NewAgentSheet } from './components/mobile/MobileSheet';
import { SetupScreen } from './components/pwa/SetupScreen';

const LS_ACTIVE_TAB = 'pm.mobile.activeTab';
const RESIZES_CONTENT = 'interactive-widget=resizes-content';

/**
 * Android Chrome: let the software keyboard shrink the page (layout viewport) instead of panning
 * it up. Only while this UI is mounted, so the legacy PWA keeps its own keyboard behaviour.
 * Browsers that ignore the hint (iOS Safari) are covered by `.pwa-shell--fit-visual`.
 */
function useKeyboardResizesContent(): void {
  useEffect(() => {
    const meta = document.querySelector<HTMLMetaElement>('meta[name="viewport"]');
    if (!meta || meta.content.includes('interactive-widget')) return;
    const original = meta.content;
    meta.content = `${original}, ${RESIZES_CONTENT}`;
    return () => {
      meta.content = original;
    };
  }, []);
}
/** Health re-check and pane list refresh: the SSE stream can drop silently on a phone. */
const RESYNC_MS = 5000;

function readActiveTab(): string {
  try {
    return localStorage.getItem(LS_ACTIVE_TAB) || ALL_TAB;
  } catch {
    return ALL_TAB;
  }
}

export default function PwaApp() {
  const [inviteCode, setInviteCode] = useState(() => readPairingInviteCode());
  const [bridgeUrl, setBridgeUrl] = useState<string | null>(() =>
    readPairingInviteCode()
      ? null
      : resolveBridgeBaseUrl(localStorage.getItem(LS_BRIDGE_URL), window.location),
  );
  const [bridge, setBridge] = useState<BridgeClient | null>(null);
  const [bridgeReady, setBridgeReady] = useState(false);
  const [panes, setPanes] = useState<PaneInfo[]>([]);
  const [panesLoaded, setPanesLoaded] = useState(false);
  const [settings, setSettings] = useState<PublicSettings>(DEFAULT_PUBLIC_SETTINGS);
  const [active, setActive] = useState<string>(readActiveTab);
  const [editingUrl, setEditingUrl] = useState(false);
  const [sheet, setSheet] = useState<'menu' | 'new' | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [spawnError, setSpawnError] = useState<string | null>(null);
  const [launchEntries, setLaunchEntries] = useState<LaunchEntry[] | null>(null);
  /** Bumped when the event stream missed events: the open terminal is rebuilt from scrollback. */
  const [resyncKey, setResyncKey] = useState(0);
  /** The grid that fits the open agent's terminal on this phone; the pane is sized to it. */
  const [fitGrid, setFitGrid] = useState<{ cols: number; rows: number } | null>(null);
  /** Live event stream handle; input prefers its socket while it is open. */
  const streamRef = useRef<BridgeEventsHandle | null>(null);
  const socketInput = useCallback(
    (paneId: string, text: string, appendNewline: boolean) =>
      streamRef.current?.sendInput(paneId, text, appendNewline) ?? false,
    [],
  );
  const registry = useBridgePaneRegistry(bridge, socketInput);
  const registryRef = useRef(registry);
  registryRef.current = registry;

  const applyPanes = useCallback((list: PaneInfo[]) => {
    setPanes(orderPanes(list));
    setPanesLoaded(true);
    registryRef.current.setPanesFromList(list);
  }, []);

  useVisualViewportSync(!!bridge);
  useKeyboardResizesContent();

  useEffect(() => {
    if (!bridgeUrl) return;
    const client = makeBridgeClient(bridgeUrl);
    setBridge(client);
    setBridgeReady(false);
    setPanesLoaded(false);
    let live = true;

    const resync = () => {
      fetch(`${bridgeUrl}/health`, {
        signal: AbortSignal.timeout(3000),
        headers: mergeBridgeHeaders(ngrokRequestHeaders(bridgeUrl)),
      })
        .then((r) => {
          if (live) setBridgeReady(r.ok);
        })
        .catch(() => {
          if (live) setBridgeReady(false);
        });
      client
        .listPanes()
        .then((list) => {
          if (live) applyPanes(list);
        })
        .catch(() => {});
    };
    resync();
    void client.getSettings().then((next) => live && setSettings(next)).catch(() => {});
    const timer = window.setInterval(resync, RESYNC_MS);

    const stream = subscribeBridgeEventsHandle(bridgeUrl, (ev) => {
      if (ev.type === 'panes') applyPanes(ev.panes);
      if (ev.type === 'terminal') registryRef.current.ingestTerminalData(ev.pane_id, ev.data);
      if (ev.type === 'pane-status') {
        registryRef.current.updatePaneStatus(ev.pane_id, ev.status);
        setPanes((prev) => prev.map((p) => (p.id === ev.pane_id ? { ...p, status: ev.status } : p)));
      }
      // Panes are sized to this phone and resized in place: keep the live stream connected.
      if (ev.type === 'pane-resize') {
        // The terminal resizes at this point of the output stream; the state update is for the header.
        registryRef.current.ingestPaneResize(ev.pane_id, ev.cols, ev.rows);
        registryRef.current.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows, true);
      }
      if (ev.type === 'settings') setSettings(ev.settings);
      if (ev.type === 'resync') {
        registryRef.current.resetStreams();
        client.listPanes().then((list) => live && applyPanes(list)).catch(() => {});
        setResyncKey((n) => n + 1);
      }
    });
    streamRef.current = stream;

    return () => {
      live = false;
      window.clearInterval(timer);
      if (streamRef.current === stream) streamRef.current = null;
      stream.unsubscribe();
    };
  }, [bridgeUrl, applyPanes]);

  // A closed agent's tab falls back to All (only once the real pane list has arrived).
  useEffect(() => {
    if (panesLoaded && active !== ALL_TAB && !panes.some((p) => p.id === active)) setActive(ALL_TAB);
  }, [active, panes, panesLoaded]);

  useEffect(() => {
    try {
      localStorage.setItem(LS_ACTIVE_TAB, active);
    } catch {
      /* private mode */
    }
  }, [active]);

  const labels = useMemo(() => tabLabels(panes), [panes]);
  const activePane = active === ALL_TAB ? undefined : registry.panes.get(active);
  const transport = useBridgePaneTransport(registry.makeTransport, activePane ? active : 'pane-placeholder');
  // Only the open agent's terminal is streamed; the All overview reads previews by polling.
  const watchedPane = activePane ? active : null;
  useEffect(() => {
    streamRef.current?.setWatchedPanes(watchedPane ? [watchedPane] : []);
  }, [watchedPane, bridgeUrl]);
  useEffect(() => setFitGrid(null), [active]);
  usePaneViewportLease(bridge, activePane ? active : null, fitGrid);

  const patchSettings = useCallback(
    (patch: Partial<PublicSettings>) => {
      if (!bridge) return;
      void bridge.patchSettings(patch).then(setSettings).catch(() => {});
    },
    [bridge],
  );

  const spawnAgent = useCallback(
    async (agentType: AgentType) => {
      if (!bridge) return;
      setSpawning(true);
      setSpawnError(null);
      try {
        const { pane_id } = await bridge.spawnPane({ agent_type: agentType, cols: 120, rows: 30 });
        applyPanes(await bridge.listPanes());
        setActive(pane_id);
        setSheet(null);
      } catch (e) {
        setSpawnError(e instanceof Error ? e.message : String(e));
      } finally {
        setSpawning(false);
      }
    },
    [bridge, applyPanes],
  );

  const openNewAgentSheet = useCallback(() => {
    setSpawnError(null);
    setSheet('new');
    if (!bridge) return;
    bridge
      .listAgentTypes()
      .then((body) => {
        const listing = listingFromBody(body);
        if (listing) setLaunchEntries(phoneLaunchEntries(listing));
        else setSpawnError('Could not read the agent list from your PC.');
      })
      .catch((e) => setSpawnError(e instanceof Error ? e.message : String(e)));
  }, [bridge]);

  const closeActive = useCallback(async () => {
    if (!bridge || active === ALL_TAB) return;
    try {
      await bridge.killPane(active);
      setActive(ALL_TAB);
      setSheet(null);
      applyPanes(await bridge.listPanes());
    } catch (e) {
      window.alert(e instanceof Error ? e.message : String(e));
    }
  }, [bridge, active, applyPanes]);

  if (inviteCode) {
    return (
      <div className="pwa-shell bg-pm-bg text-zinc-100">
        <PairInviteScreen
          pairingCode={inviteCode}
          onConnected={(url) => {
            clearPairingInviteFromUrl();
            setInviteCode(null);
            setBridgeUrl(url);
            setEditingUrl(false);
          }}
          onGiveUp={() => {
            clearPairingInviteFromUrl();
            setInviteCode(null);
          }}
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
    <div className="pwa-shell pwa-shell--fit-visual bg-pm-bg text-zinc-100">
      <div className="flex h-full min-h-0 min-w-0 flex-col">
        <AgentTabBar
          panes={panes}
          labels={labels}
          active={active}
          bridgeReady={bridgeReady}
          onSelect={setActive}
          onNew={openNewAgentSheet}
          onMenu={() => setSheet('menu')}
        />
        <main className="relative flex min-h-0 flex-1 flex-col bg-pm-panel" aria-label="Agent">
          {bridge && active === ALL_TAB && (
            <AgentsOverview
              bridge={bridge}
              bridgeReady={bridgeReady}
              panes={panes}
              labels={labels}
              onOpen={setActive}
              onNew={openNewAgentSheet}
            />
          )}
          {activePane && (
            <BridgePaneTerminal
              key={`${active}:${resyncKey}`}
              pane={activePane.info}
              status={activePane.status}
              subscribePaneData={registry.subscribePaneData}
              transport={transport}
              title={`${labels.get(active) ?? activePane.info.agent_type}`}
              mobileInputDelayMs={settings.mobile_input_delay_ms}
              mobileInputVisible={settings.mobile_input_visible}
              onFitGrid={setFitGrid}
              keyboardResizesAgent={settings.mobile_keyboard_resizes_agent ?? false}
            />
          )}
          {active !== ALL_TAB && !activePane && (
            <div className="py-10 text-center text-xs text-pm-muted">Loading agent…</div>
          )}
        </main>
      </div>
      {sheet === 'new' && (
        <NewAgentSheet
          entries={launchEntries}
          onClose={() => setSheet(null)}
          onSpawn={(type) => void spawnAgent(type)}
          spawning={spawning}
          error={spawnError}
        />
      )}
      {sheet === 'menu' && (
        <MenuSheet
          onClose={() => setSheet(null)}
          bridgeUrl={bridgeUrl}
          bridgeReady={bridgeReady}
          settings={settings}
          onPatchSettings={patchSettings}
          activeLabel={activePane ? labels.get(active) ?? activePane.info.agent_type : null}
          onCloseActive={() => void closeActive()}
          onEditConnection={() => {
            setEditingUrl(true);
            setSheet(null);
          }}
        />
      )}
    </div>
  );
}
