// Mobile PWA root: watch and reply to the agents running on the PC, one tab per agent pane (like
// terminal tabs) plus an "All" overview. It does not drive the orchestrator. The previous
// orchestrator-centric mobile UI is kept as PwaLegacyApp.tsx (open with `?pwa=legacy`).
import { NudgeScheduler } from './lib/nudge-scheduler';
import { newerSnapshot, type AttentionItem, type AttentionSnapshot } from './lib/attention';
import { COPY } from './lib/attention-copy';
import { PROMPT_KINDS } from './lib/needs-you-queue';
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
import { fallbackListing, listingFromBody, type AgentRegistryListing, type LaunchEntry } from './lib/agentRegistry';
import { ALL_TAB, orderPanes, phoneLaunchEntries, tabLabels } from './lib/mobile-agent-tabs';
import {
  PairInviteScreen,
  readPairingInviteCode,
  clearPairingInviteFromUrl,
} from './components/PairInviteScreen';
import { BridgePaneTerminal, useBridgePaneTransport } from './components/BridgePaneTerminal';
import { AgentTabBar } from './components/mobile/AgentTabBar';
import { PreviewNote } from './components/mobile/PreviewNote';
import { StuckNote } from './components/mobile/StuckNote';
import type { LoopSignalInfo } from './lib/bridge-types';
import './styles/phone.css';
import type { PreviewUrl } from './lib/pane-preview';
import { AgentsOverview } from './components/mobile/AgentsOverview';
import { DockedCard } from './components/mobile/DockedCard';
import { CardSheet } from './components/mobile/CardSheet';
import { usePushIntents } from './hooks/usePushIntents';
import { usePhoneAbilities } from './hooks/usePhoneAbilities';
import { inputRefusalNote } from './lib/input-refusal';
import { InputRefusedNote } from './components/mobile/InputRefusedNote';
import { TypingNote } from './components/mobile/TypingNote';
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
  /** Panes the loop guard flagged as stuck (event `loop-signal`). */
  const [stuck, setStuck] = useState<Map<string, LoopSignalInfo>>(new Map());
  /** Short confirmation shown at the bottom (replaces browser alerts). */
  const [toast, setToast] = useState<string | null>(null);
  const [settings, setSettings] = useState<PublicSettings>(DEFAULT_PUBLIC_SETTINGS);
  const [active, setActive] = useState<string>(readActiveTab);
  const [editingUrl, setEditingUrl] = useState(false);
  const [sheet, setSheet] = useState<'menu' | 'new' | null>(null);
  const [spawning, setSpawning] = useState(false);
  const [spawnError, setSpawnError] = useState<string | null>(null);
  const [launchEntries, setLaunchEntries] = useState<LaunchEntry[] | null>(null);
  /** The agent registry from the PC: display names for tabs and rows (compiled agents until it answers). */
  const [agentListing, setAgentListing] = useState<AgentRegistryListing>(() => fallbackListing());
  /** Bumped when the event stream missed events: the open terminal is rebuilt from scrollback. */
  const [resyncKey, setResyncKey] = useState(0);
  /** Bumped on each `questions` event (and on resync) so the pending-questions list refetches. */
  const [questionsRev, setQuestionsRev] = useState(0);
  /** The PC's one list of what needs you (event `attention`, `GET /attention`); null on an old PC. */
  const [attention, setAttention] = useState<AttentionSnapshot | null>(null);
  /** A tapped notification points at this card; it is looked up on the PC's list once the bridge is up. */
  const [intent, setIntent] = useState<{ itemId: string; paneId?: string | null } | null>(null);
  /** A card shown full screen (an item with no pane to open, or one that already left the list). */
  const [sheetItem, setSheetItem] = useState<AttentionItem | null>(null);
  /** The card a push tap opened is docked: its focus choice takes the keyboard focus once. */
  const [deepLinked, setDeepLinked] = useState(false);
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
  // Offsets on the live stream show output that never arrived (see PaneStreamManager `onGap`): heal
  // it with the nudge, which redraws the open agent from the PC's current screen.
  const streamGapRef = useRef<() => void>(() => {});
  /** The PC refused what was typed: a plain line under the terminal (null = nothing refused lately). */
  const [inputRefused, setInputRefused] = useState<{ paneId: string; text: string } | null>(null);
  const noteInputRefused = useCallback((paneId: string, detail: unknown) => {
    setInputRefused({ paneId, text: inputRefusalNote(detail) });
  }, []);
  const registry = useBridgePaneRegistry(bridge, socketInput, () => streamGapRef.current(), noteInputRefused);
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

    const refreshAttention = () => {
      void client.attention?.().then((s) => live && setAttention((cur) => newerSnapshot(cur, s))).catch(() => {});
    };
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
      refreshAttention();
    };
    resync();
    void client.getSettings().then((next) => live && setSettings(next)).catch(() => {});
    void client
      .listAgentTypes()
      .then((body) => {
        const listing = listingFromBody(body);
        if (live && listing) setAgentListing(listing);
      })
      .catch(() => {});
    const timer = window.setInterval(resync, RESYNC_MS);

    const stream = subscribeBridgeEventsHandle(bridgeUrl, (ev) => {
      if (ev.type === 'panes') applyPanes(ev.panes);
      if (ev.type === 'terminal') {
        registryRef.current.ingestTerminalData(ev.pane_id, ev.data, ev.offset);
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
      if (ev.type === 'loop-signal') {
        setStuck((cur) => {
          const next = new Map(cur);
          if (ev.signal) next.set(ev.pane_id, ev.signal);
          else next.delete(ev.pane_id);
          return next;
        });
      }
      if (ev.type === 'pane-status') {
        registryRef.current.updatePaneStatus(ev.pane_id, ev.status);
        setPanes((prev) => prev.map((p) => (p.id === ev.pane_id ? { ...p, status: ev.status } : p)));
      }
      // Panes are sized to this phone and resized in place: keep the live stream connected.
      if (ev.type === 'pane-resize') {
        // The terminal resizes at this point of the output stream; the state update is for the header.
        registryRef.current.ingestPaneResize(ev.pane_id, ev.cols, ev.rows, ev.offset);
        registryRef.current.updatePaneDimensions(ev.pane_id, ev.cols, ev.rows, true);
      }
      if (ev.type === 'input-refused') noteInputRefused(ev.pane_id, ev.error);
      if (ev.type === 'settings') setSettings(ev.settings);
      if (ev.type === 'questions') setQuestionsRev((n) => n + 1);
      if (ev.type === 'attention') setAttention((cur) => newerSnapshot(cur, ev.snapshot));
      if (ev.type === 'resync') {
        registryRef.current.resetStreams();
        client.listPanes().then((list) => live && applyPanes(list)).catch(() => {});
        setResyncKey((n) => n + 1);
        setQuestionsRev((n) => n + 1);
        refreshAttention();
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
  }, [bridgeUrl, applyPanes, publishEcho, noteInputRefused]);

  usePushIntents(bridgeUrl, {
    onOpenItem: (itemId, paneId) => setIntent({ itemId, paneId }),
    onOpenPane: setActive,
    onOpenQuestions: () => setActive(ALL_TAB),
  });

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

  const abilities = usePhoneAbilities(bridge, resyncKey);
  /** Read the PC's list now (an answer came back stale), instead of waiting for the next poll. */
  const refreshAttentionNow = useCallback(() => {
    void bridge?.attention?.().then((s) => setAttention((cur) => newerSnapshot(cur, s))).catch(() => {});
  }, [bridge]);
  // What the PC's list says this phone may do wins over the pairing read (it comes with every list read).
  const canAnswer = attention?.device?.can_answer ?? abilities.canAnswer;

  // A push tap on a card: read the list first (before the terminal stream is opened), then show it.
  useEffect(() => {
    if (!intent || !bridge) return;
    let live = true;
    void (async () => {
      let snapshot = attention;
      try {
        const fresh = await bridge.attention?.();
        if (fresh) {
          snapshot = newerSnapshot(attention, fresh);
          if (live) setAttention((cur) => newerSnapshot(cur, fresh));
        }
      } catch {
        /* the stream will bring the list; use what is held */
      }
      if (!live) return;
      const item = snapshot?.items.find((i) => i.id === intent.itemId);
      if (item) {
        if (item.pane && PROMPT_KINDS.includes(item.kind)) {
          setActive(item.pane.id);
          setDeepLinked(true);
        } else {
          setSheetItem(item);
        }
      } else {
        // Older than the 15 s the list keeps what just left: say so and still show the way.
        setActive(intent.paneId ?? ALL_TAB);
        setToast(COPY.dealtWith);
      }
      setIntent(null);
    })();
    return () => {
      live = false;
    };
    // The list at the time of the tap is read inside; it must not restart the lookup.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [intent, bridge]);

  const labels = useMemo(() => tabLabels(panes, agentListing), [panes, agentListing]);
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
  const nudgeSchedulerRef = useRef<NudgeScheduler | null>(null);
  if (!nudgeSchedulerRef.current) {
    nudgeSchedulerRef.current = new NudgeScheduler((skip) =>
      setRefreshSignal((prev) => ({ n: prev.n + 1, skipIfScrolledUp: skip })),
    );
  }
  const requestNudge = useCallback((skipIfScrolledUp: boolean) => {
    nudgeSchedulerRef.current?.request(skipIfScrolledUp);
  }, []);
  streamGapRef.current = () => requestNudge(true);
  useEffect(() => () => nudgeSchedulerRef.current?.cancel(), []);
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
        if (listing) {
          setAgentListing(listing);
          setLaunchEntries(phoneLaunchEntries(listing));
        }
        else setSpawnError('Could not read the agent list from your PC.');
      })
      .catch((e) => setSpawnError(e instanceof Error ? e.message : String(e)));
  }, [bridge]);

  // The refusal line stays while the person is looking at it, then goes (typing again brings it back).
  useEffect(() => {
    if (!inputRefused) return;
    const t = window.setTimeout(() => setInputRefused(null), 12_000);
    return () => window.clearTimeout(t);
  }, [inputRefused]);

  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(t);
  }, [toast]);

  const closeActive = useCallback(async () => {
    if (!bridge || active === ALL_TAB) return;
    try {
      await bridge.killPane(active);
      setActive(ALL_TAB);
      setSheet(null);
      applyPanes(await bridge.listPanes());
    } catch (e) {
      setToast(e instanceof Error ? e.message : String(e));
    }
  }, [bridge, active, applyPanes]);

  if (inviteCode) {
    return (
      <div className="pwa-shell ph bg-pm-bg text-zinc-100">
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
      <div className="pwa-shell ph bg-pm-bg text-zinc-100">
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
    <div className="pwa-shell pwa-shell--fit-visual ph">
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
        {active !== ALL_TAB && stuck.get(active) && <StuckNote signal={stuck.get(active)!} />}
        {active !== ALL_TAB && previews[active]?.[0] && <PreviewNote preview={previews[active][0]} />}
        <main
          className="ph-main relative flex min-h-0 flex-1 flex-col"
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
              stuck={stuck}
              questionsRevision={questionsRev}
              attention={attention}
              canAnswer={canAnswer}
              onRefreshAttention={refreshAttentionNow}
            />
          )}
          {bridge && activePane && attention && (
            <DockedCard
              snapshot={attention}
              paneId={active}
              bridge={bridge}
              canAnswer={canAnswer}
              autoFocus={deepLinked}
              onRefresh={refreshAttentionNow}
              onOpenPane={setActive}
              onOpenItem={(id) => {
                const next = attention.items.find((i) => i.id === id);
                if (next) setSheetItem(next);
              }}
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
              disableMobileInput={!abilities.canType}
              onFitGrid={setFitGrid}
              refreshSignal={refreshSignal}
              keyboardResizesAgent={settings.mobile_keyboard_resizes_agent ?? false}
            />
          )}
          {active !== ALL_TAB && !activePane && (
            <div className="ph-loading">Loading agent…</div>
          )}
          {activePane && abilities.typingNote && <TypingNote text={abilities.typingNote} />}
          {activePane && inputRefused && inputRefused.paneId === active && <InputRefusedNote text={inputRefused.text} />}
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
              .then((r) => setToast(r.stopped.length === 0 ? 'Nothing was running.' : `Stopped ${r.stopped.length} agent${r.stopped.length === 1 ? '' : 's'}.`))
              .catch((e) => setToast(e instanceof Error ? e.message : String(e)));
          }}
          onEditConnection={() => {
            setEditingUrl(true);
            setSheet(null);
          }}
        />
      )}
      {bridge && sheetItem && (
        <CardSheet
          item={sheetItem}
          snapshot={attention ?? { v: 1, rev: 0, counts: { blocking: 0, ready: 0 }, items: [sheetItem] }}
          bridge={bridge}
          canAnswer={canAnswer}
          labels={labels}
          onRefresh={refreshAttentionNow}
          onClose={() => setSheetItem(null)}
          onOpenPane={(id) => {
            setSheetItem(null);
            setActive(id);
          }}
        />
      )}
      {toast && (
        <div className="ph-toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
