// Mobile PWA root: watch and reply to the agents running on the PC, one tab per agent pane (like
// terminal tabs) plus an "All" overview. It does not drive the orchestrator. The previous
// orchestrator-centric mobile UI is kept as PwaLegacyApp.tsx (open with `?pwa=legacy`).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AgentType, PaneInfo } from '@puppet-master/shared';
import { useBridgePaneRegistry } from './hooks/useBridgePaneRegistry';
import type { TerminalTransport } from './hooks/useTerminalSession';
import { usePaneViewportLease, VIEWER_ID } from './hooks/usePaneViewportLease';
import { useVisualViewportSync } from './hooks/useVisualViewport';
import { makeBridgeClient, type BridgeClient } from './lib/bridge';
import { subscribeBridgeEventsHandle, type BridgeEventsHandle, type BridgeLinkStats } from './lib/bridge-sse';
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
import { PreviewNote } from './components/mobile/PreviewNote';
import type { PreviewUrl } from './lib/pane-preview';
import { AgentsOverview } from './components/mobile/AgentsOverview';
import { QuestionsList } from './components/mobile/QuestionsList';
import { MenuSheet, NewAgentSheet, type TypingEchoStats } from './components/mobile/MobileSheet';
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
/** Typing-echo samples kept (median over these). */
const ECHO_WINDOW = 10;
/** A keystroke still waiting for its echo after this long is dropped (no output came back). */
const ECHO_PENDING_MAX_MS = 3000;
/** Echo stats are published to state at most this often. */
const ECHO_PUBLISH_MS = 1000;

function medianOf(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

const LS_LAST_FIT_GRID = 'pm.mobile.lastFitGrid';
/** Nudge timing: wait this long after the last action (lets a size claim land first)... */
const NUDGE_SETTLE_MS = 350;
/** ...and never redraw more often than this. */
const NUDGE_MIN_GAP_MS = 1500;
/** A touch that moves less than this and lifts within TAP_MAX_MS is a tap, not a swipe. */
const TAP_MAX_MOVE_PX = 10;
const TAP_MAX_MS = 500;

/** The last grid that fit this phone's terminal area, so a spawn right after a reload is phone-sized. */
function readLastFitGrid(): { cols: number; rows: number } | null {
  try {
    const parsed = JSON.parse(localStorage.getItem(LS_LAST_FIT_GRID) ?? 'null') as { cols?: unknown; rows?: unknown } | null;
    if (parsed && typeof parsed.cols === 'number' && typeof parsed.rows === 'number' && parsed.cols > 0 && parsed.rows > 0) {
      return { cols: parsed.cols, rows: parsed.rows };
    }
  } catch {
    /* private mode or bad value */
  }
  return null;
}

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
  const [previews, setPreviews] = useState<Record<string, PreviewUrl[]>>({});
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
  /** Last grid measured on this phone: a pane spawned from here is created at this size. */
  const lastFitGridRef = useRef<{ cols: number; rows: number } | null>(readLastFitGrid());
  /** A pane this phone spawned: it takes the phone's size as soon as its fitting grid is measured. */
  const claimOnOpenRef = useRef<string | null>(null);
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
  const [linkStats, setLinkStats] = useState<BridgeLinkStats | null>(null);
  const [echoStats, setEchoStats] = useState<TypingEchoStats | null>(null);
  /** Keystroke waiting for the pane's next output. Refs only; state is published at most once a second. */
  const echoPendingRef = useRef<{ paneId: string; at: number } | null>(null);
  const echoSamplesRef = useRef<number[]>([]);
  const echoLastRef = useRef<number | null>(null);
  const echoTimerRef = useRef<number | null>(null);
  const publishEcho = useCallback(() => {
    if (echoTimerRef.current !== null) return;
    echoTimerRef.current = window.setTimeout(() => {
      echoTimerRef.current = null;
      setEchoStats({ lastMs: echoLastRef.current, medianMs: medianOf(echoSamplesRef.current) });
    }, ECHO_PUBLISH_MS);
  }, []);
  useEffect(
    () => () => {
      if (echoTimerRef.current !== null) window.clearTimeout(echoTimerRef.current);
    },
    [],
  );

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
      void client.listPreviews?.().then((m) => live && setPreviews(m)).catch(() => {});
    };
    resync();
    void client.getSettings().then((next) => live && setSettings(next)).catch(() => {});
    const timer = window.setInterval(resync, RESYNC_MS);

    const stream = subscribeBridgeEventsHandle(bridgeUrl, (ev) => {
      if (ev.type === 'panes') applyPanes(ev.panes);
      if (ev.type === 'terminal') {
        registryRef.current.ingestTerminalData(ev.pane_id, ev.data);
        const pending = echoPendingRef.current;
        if (pending && pending.paneId === ev.pane_id) {
          echoPendingRef.current = null;
          echoLastRef.current = performance.now() - pending.at;
          echoSamplesRef.current.push(echoLastRef.current);
          if (echoSamplesRef.current.length > ECHO_WINDOW) echoSamplesRef.current.shift();
          publishEcho();
        }
      }
      if (ev.type === 'pane-preview') {
        setPreviews((cur) => {
          if (ev.urls.length > 0) return { ...cur, [ev.pane_id]: ev.urls };
          const { [ev.pane_id]: _gone, ...rest } = cur;
          return rest;
        });
      }
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
    setLinkStats(stream.stats());
    const offStats = stream.onStats(setLinkStats);

    return () => {
      live = false;
      window.clearInterval(timer);
      offStats();
      if (streamRef.current === stream) streamRef.current = null;
      stream.unsubscribe();
    };
  }, [bridgeUrl, applyPanes, publishEcho]);

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
  const claimPaneSize = usePaneViewportLease(bridge, activePane ? active : null, fitGrid);
  useEffect(() => {
    if (!fitGrid) return;
    lastFitGridRef.current = fitGrid;
    try {
      localStorage.setItem(LS_LAST_FIT_GRID, JSON.stringify(fitGrid));
    } catch {
      /* private mode */
    }
  }, [fitGrid]);
  // Any phone action takes the size: opening the PWA, switching to a tab, bringing the app back
  // to the front (and spawning, typing, touching the terminal below). Claimed once the tab's
  // fitting grid is known; the PC window takes it back on a click or focus there.
  useEffect(() => {
    if (active !== ALL_TAB) claimOnOpenRef.current = active;
  }, [active]);
  // Nudge: after a phone action (not typing) redraw the open agent from the PC's current screen,
  // in place. Actions close together share one redraw; at most one per NUDGE_MIN_GAP_MS. A tap
  // while reading history above the screen does not jump back to the bottom.
  const [refreshSignal, setRefreshSignal] = useState({ n: 0, skipIfScrolledUp: false });
  const nudgeTimerRef = useRef(0);
  const lastNudgeAtRef = useRef(0);
  const nudgeSkipRef = useRef(true);
  const requestNudge = useCallback((skipIfScrolledUp: boolean) => {
    nudgeSkipRef.current = nudgeSkipRef.current && skipIfScrolledUp;
    window.clearTimeout(nudgeTimerRef.current);
    const wait = Math.max(NUDGE_SETTLE_MS, NUDGE_MIN_GAP_MS - (Date.now() - lastNudgeAtRef.current));
    nudgeTimerRef.current = window.setTimeout(() => {
      lastNudgeAtRef.current = Date.now();
      const skip = nudgeSkipRef.current;
      nudgeSkipRef.current = true;
      setRefreshSignal((prev) => ({ n: prev.n + 1, skipIfScrolledUp: skip }));
    }, wait);
  }, []);
  useEffect(() => () => window.clearTimeout(nudgeTimerRef.current), []);
  const tapRef = useRef<{ x: number; y: number; t: number } | null>(null);
  const onAgentPointerDown = useCallback(
    (event: React.PointerEvent) => {
      claimPaneSize(true);
      tapRef.current = { x: event.clientX, y: event.clientY, t: Date.now() };
    },
    [claimPaneSize],
  );
  const onAgentPointerUp = useCallback(
    (event: React.PointerEvent) => {
      const start = tapRef.current;
      tapRef.current = null;
      if (!start || Date.now() - start.t > TAP_MAX_MS) return;
      if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > TAP_MAX_MOVE_PX) return;
      // The Keys panel, the input box and the jump-to-bottom button are typing / their own actions.
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest('[data-mobile-terminal-arrows],[data-mobile-terminal-input-zone],[data-mobile-terminal-input],[data-terminal-scroll-bottom]')) return;
      requestNudge(true);
    },
    [requestNudge],
  );
  const selectTab = useCallback(
    (id: string) => {
      setActive(id);
      if (id !== ALL_TAB) requestNudge(false);
    },
    [requestNudge],
  );

  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState !== 'visible' || active === ALL_TAB) return;
      requestNudge(false);
      claimOnOpenRef.current = active;
      if (fitGrid) {
        claimOnOpenRef.current = null;
        claimPaneSize(true);
      }
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  }, [active, fitGrid, claimPaneSize, requestNudge]);
  useEffect(() => {
    if (!fitGrid || !activePane || claimOnOpenRef.current !== active) return;
    claimOnOpenRef.current = null;
    claimPaneSize(true);
  }, [fitGrid, activePane, active, claimPaneSize]);
  const leasedTransport = useMemo<TerminalTransport>(
    () => ({
      ...transport,
      writeInput: (text, appendNewline) => {
        claimPaneSize();
        const now = performance.now();
        const pending = echoPendingRef.current;
        if (!pending || now - pending.at > ECHO_PENDING_MAX_MS) echoPendingRef.current = { paneId: active, at: now };
        return transport.writeInput(text, appendNewline);
      },
    }),
    [transport, claimPaneSize, active],
  );

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
        // Known phone grid (kept across reloads): spawn at it and hold it from the start.
        const grid = lastFitGridRef.current;
        const { pane_id } = await bridge.spawnPane(
          grid
            ? { agent_type: agentType, cols: grid.cols, rows: grid.rows, viewer: VIEWER_ID }
            : { agent_type: agentType, cols: 120, rows: 30 },
        );
        claimOnOpenRef.current = pane_id;
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
          onSelect={selectTab}
          onNew={openNewAgentSheet}
          onMenu={() => setSheet('menu')}
        />
        {active !== ALL_TAB && previews[active]?.[0] && <PreviewNote preview={previews[active][0]} />}
        <main
          className="relative flex min-h-0 flex-1 flex-col bg-pm-panel"
          aria-label="Agent"
          onPointerDownCapture={activePane ? onAgentPointerDown : undefined}
          onPointerUpCapture={activePane ? onAgentPointerUp : undefined}
        >
          {bridge && active === ALL_TAB && (
            <AgentsOverview
              bridge={bridge}
              bridgeReady={bridgeReady}
              panes={panes}
              labels={labels}
              onOpen={setActive}
              onNew={openNewAgentSheet}
              header={<QuestionsList bridge={bridge} bridgeReady={bridgeReady} labels={labels} />}
            />
          )}
          {activePane && (
            <BridgePaneTerminal
              key={`${active}:${resyncKey}`}
              pane={activePane.info}
              status={activePane.status}
              subscribePaneData={registry.subscribePaneData}
              transport={leasedTransport}
              title={`${labels.get(active) ?? activePane.info.agent_type}`}
              mobileInputDelayMs={settings.mobile_input_delay_ms}
              mobileInputVisible={settings.mobile_input_visible}
              onFitGrid={setFitGrid}
              refreshSignal={refreshSignal}
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
          link={linkStats}
          echo={echoStats}
          activeLabel={activePane ? labels.get(active) ?? activePane.info.agent_type : null}
          onCloseActive={() => void closeActive()}
          onStopAll={() => {
            if (!bridge) return;
            bridge
              .stopAllPanes()
              .then((r) => window.alert(r.stopped.length === 0 ? 'Nothing was running.' : `Stopped ${r.stopped.length} agent${r.stopped.length === 1 ? '' : 's'}.`))
              .catch((e) => window.alert(e instanceof Error ? e.message : String(e)));
          }}
          onEditConnection={() => {
            setEditingUrl(true);
            setSheet(null);
          }}
        />
      )}
    </div>
  );
}
