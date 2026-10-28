// Cockpit window (`index.html?cockpit=1`): one window that orchestrates every agent pane.
import { useNudgeOnAction } from './lib/desktop-nudge';
import { useEffect, useRef, useState } from 'react';
import { AgentDoctor } from './components/cockpit/AgentDoctor';
import { markSpawned } from './lib/agent-health';
import { useCockpit } from './hooks/useCockpit';
import { useAway } from './hooks/useAway';
import { LastTurnContext } from './components/cockpit/away-context';
import { AwayDigest } from './components/cockpit/AwayDigest';
import { Inbox } from './components/cockpit/Inbox';
import { PaneView } from './components/cockpit/PaneView';
import { Palette, type PaletteMode } from './components/cockpit/Palette';
import { NeedsYouContext, useNeedsYouSource } from './hooks/useNeedsYou';
import { useMissions } from './lib/missions';
import { UndoSheet } from './components/cockpit/UndoSheet';
import { panesSharingFolder } from './components/cockpit/history-model';
import { ProjectMemory } from './components/cockpit/ProjectMemory';
import { Sidebar } from './components/cockpit/Sidebar';
import { SettingsView } from './components/cockpit/SettingsView';
import { PlansMonth } from './components/cockpit/PlansMonth';
import { MorningPage } from './components/cockpit/night/MorningPage';
import { AgentScores } from './components/cockpit/AgentScores';
import { StoreHost } from './components/store/StoreHost';
import { useInlineSettings } from './hooks/useInlineSettings';
import { SpawnView } from './components/cockpit/SpawnView';
import { PanicSheet } from './components/cockpit/PanicSheet';
import { TellTeamToast } from './components/cockpit/team/TellTeam';
import { SimpleCockpit } from './components/simple/SimpleCockpit';
import { FirstRun } from './components/firstrun/FirstRun';
import { useFirstRun } from './components/firstrun/useFirstRun';
import { useSimpleMode } from './lib/simple-mode';
import { projectOfCwd } from './lib/ship-model';
import { LoopGuardContext, tauriLoopApi, useLoopSignals, type StopReport } from './lib/loop-guard';
import { homeView, isMacPlatform, isSplitShortcut, splitAfterSelect, splitFits, splitFocusKey, splitPartner, splitStillOn, SPLIT_DEFAULT_RATIO, viewAfterSelect, type MainView, type SplitSide } from './components/cockpit/main-view';
import { focusSideComposer, SplitView, useWindowWidth } from './components/cockpit/SplitView';
import { clearSplitMemory, readSplitMemory, restorableSplit, writeSplitMemory } from './components/cockpit/split-memory';
import { useCompanions } from './hooks/useCompanions';
import { tauriCompanionTransport } from './lib/companion';
import { CompanionPage } from './components/cockpit/companion/CompanionPage';
import { COMPANION_PANE_PREFIX, isCompanionPaneId } from './components/cockpit/companion/model';
import { CompanionSetupSheet } from './components/cockpit/companion/CompanionSetupSheet';
import { StartCrewSheet } from './components/cockpit/crews/StartCrewSheet';
import { ShortcutsSheet } from './components/cockpit/ShortcutsSheet';
import { ImportHost } from './components/history/ImportHost';
import { isTypingTarget } from './components/cockpit/rail/useRailLayout';
import { agentLabel, useAgentRegistry } from './lib/agentRegistry';
import { LiveBar } from './components/cockpit/live/LiveBar';
import { GoLiveSheet } from './components/cockpit/live/GoLiveSheet';
import { LiveStatusContext, useLiveRoom } from './components/cockpit/live/useLiveRoom';
import { useTeamRelay } from './components/cockpit/live/useTeamRelay';
import { sharePanes } from './components/cockpit/live/sharePanes';
import { useLiveStopToast } from './components/cockpit/live/stopToast';
import { GO_LIVE_EVENT } from './lib/live-room';
import './styles/cockpit.css';
import './styles/night.css';
import './styles/night-morning.css';
import './styles/night-chip.css';

export type { MainView } from './components/cockpit/main-view';

/** True inside a pane's terminal, where `?` is typed text. */
const inTerminal = (target: EventTarget | null): boolean => target instanceof Element && target.closest('.terminal-host, .pm-canvas-terminal') !== null;

export default function CockpitApp() {
  const c = useCockpit();
  const [paletteOpen, setPaletteOpen] = useState(false);
  // Ctrl+Shift+F opens the palette in "Search all chats"; Ctrl+K in its commands.
  const [paletteMode, setPaletteMode] = useState<PaletteMode>('commands');
  // The one thing the main area shows (see main-view.ts): the last click wins.
  const [view, setView] = useState<MainView>(() => homeView(c.spawn !== null, c.selectedId));
  // A new selection or spawn state (a pane closing, an agent starting, a digest opening a pane)
  // returns to the resting view. Done while rendering, so no frame shows a stale view.
  const [seen, setSeen] = useState({ spawn: c.spawn, selectedId: c.selectedId });
  const [companionId, setCompanionId] = useState<string | null>(null);
  if (seen.spawn !== c.spawn || seen.selectedId !== c.selectedId) {
    setSeen({ spawn: c.spawn, selectedId: c.selectedId });
    // A companion's pane closes and starts again by itself (a move, a restart, a nap). Whoever was looking at it
    // lands back on the companion's page, not on the Inbox.
    const lostCompanionPane = c.spawn === null && c.selectedId === null && seen.selectedId !== null && isCompanionPaneId(seen.selectedId);
    if (lostCompanionPane) {
      setCompanionId(seen.selectedId!.slice(COMPANION_PANE_PREFIX.length));
      setView('companion');
    } else {
      setView(homeView(c.spawn !== null, c.selectedId));
    }
  }
  // The split view (main-view.ts): the pane on the right, and which side has focus. The left side is the selected pane.
  const [split, setSplit] = useState<string | null>(null);
  const [side, setSide] = useState<SplitSide>('left');
  // The left side's share of the room (drag or arrow keys on the divider). Remembered with the split (split-memory.ts).
  const [ratio, setRatio] = useState(SPLIT_DEFAULT_RATIO);
  const width = useWindowWidth();
  const paneIds = c.registry.paneList.map((p) => p.info.id);
  const splitOn = splitStillOn(split, { selectedId: c.selectedId, width, hasPane: (id) => c.registry.panes.has(id) });
  // The split turns off by itself when its pane closes, the selection goes away, or the window gets narrow.
  if (split !== null && !splitOn) setSplit(null);
  const closeOverlay = () => setView(homeView(c.spawn !== null, c.selectedId));
  // The Agent Store: opened from the "+" menu, the palette, Set up agents and Settings > Agents. Back returns to the resting view.
  const openStore = () => setView('store');
  // Settings is a main view in Orchestration mode; in Individual mode `open` asks for the Settings window.
  const settings = useInlineSettings<MainView>({ mode: c.mode, view, setView, home: homeView(c.spawn !== null, c.selectedId), openWindow: c.openSettings });
  // Pane whose last turn the person is being asked to undo (opened from the palette).
  const [undoPane, setUndoPane] = useState<string | null>(null);
  const loop = useLoopSignals();
  // Companions (SPEC section 8): the sidebar rows, the page that opens from one, and the setup sheet. Without a
  // companion backend the list is null and the palette and sidebar show no companion entries.
  const companions = useCompanions();
  const [setupOpen, setSetupOpen] = useState(false);
  // "Start crew…" (crews): pick a saved team and a folder.
  const [crewOpen, setCrewOpen] = useState(false);
  // The keyboard shortcuts sheet: `?`, Ctrl+/, or "Keyboard shortcuts" in the palette.
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const agentListing = useAgentRegistry();
  // Live Room: one status for the bar, the pane chips and the palette. The Go live sheet opens on an event the chat
  // menu and the palette fire. The stop toast says why a room ended, whoever ended it.
  const live = useLiveRoom();
  const [goLive, setGoLive] = useState<{ paneId: string | null } | null>(null);
  const teamRelay = useTeamRelay(goLive !== null);
  useLiveStopToast(live.status, c.notify);
  useEffect(() => {
    const onGoLive = (e: Event) => {
      if (live.status?.live) {
        c.notify('Live Room is already running. Stop it first.');
        return;
      }
      setGoLive({ paneId: (e as CustomEvent<{ paneId: string | null }>).detail?.paneId ?? null });
    };
    window.addEventListener(GO_LIVE_EVENT, onGoLive);
    return () => window.removeEventListener(GO_LIVE_EVENT, onGoLive);
  });
  const companion = companions.companions?.find((x) => x.id === companionId) ?? null;
  const openCompanion = (id: string) => {
    setCompanionId(id);
    setView('companion');
  };
  const [stopReport, setStopReport] = useState<StopReport | null>(null);
  const stopAll = () => {
    tauriLoopApi
      .stopAll()
      .then(setStopReport)
      .catch(() => c.notify('Could not stop the agents'));
  };
  // Tray item and global shortcut stop the agents in Rust and send the result here.
  useEffect(() => {
    let off: (() => void) | undefined;
    let disposed = false;
    tauriLoopApi
      .onPanic(setStopReport)
      .then((u) => (disposed ? u() : (off = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      off?.();
    };
  }, []);
  // Any selection (a pane, the inbox), even of the already-selected pane, leaves the
  // "Set up agents" and "Project memory" views.
  const nav: typeof c = {
    ...c,
    openSettings: async (tab) => settings.open(tab),
    select: (id) => {
      setView(viewAfterSelect(c.spawn, id));
      setSplit((s) => splitAfterSelect(s, c.selectedId, id));
      setSide('left');
      c.select(id);
    },
  };
  // Ctrl+\: split with the pane above (or below) the selected one; again closes it.
  const toggleSplit = () => {
    if (split !== null) {
      setSplit(null);
      setSide('left');
      return;
    }
    if (!splitFits(width)) return;
    if (c.selectedId === null) {
      c.notify('Pick an agent first to split the view');
      return;
    }
    const partner = splitPartner(paneIds, c.selectedId);
    if (partner === null) {
      c.notify('Open a second agent to split the view');
      return;
    }
    setView('pane');
    setSplit(partner);
    setSide('left');
  };
  // "Open to the side" in a pane row's menu: the row's pane goes on the right, the selected one stays left.
  const openToSide = (id: string) => {
    if (!splitFits(width)) {
      c.notify('The window is too narrow for two chats');
      return;
    }
    if (c.selectedId === null) {
      nav.select(id);
      return;
    }
    if (id === c.selectedId) return;
    setView('pane');
    setSplit(id);
    setSide('left');
  };
  useEffect(() => {
    if (c.registry.paneList.length > 0) markSpawned();
  }, [c.registry.paneList.length]);
  const away = useAway(c.registry.paneList.map((p) => p.info.id));
  // The first-run welcome: once on a fresh profile, again from the palette or Settings.
  const welcome = useFirstRun(c.registry.paneList.length, c.registry.initialReady);
  // One welcome: the Inbox's "Set up your agents" opens the first-run flow until it was finished or
  // skipped (Set up agents after), and while the flow is on screen the Inbox offers no second way in.
  const welcomeShown = welcome.open && c.mode !== 'individual';
  const welcomeSetUp = welcomeShown ? null : !welcome.done && c.mode !== 'individual' ? welcome.resume : () => setView('doctor');

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (!e.shiftKey && key === 'k') {
        e.preventDefault();
        setPaletteMode('commands');
        setPaletteOpen((v) => !v);
      } else if (e.shiftKey && key === 'f') {
        e.preventDefault();
        setPaletteMode('chats');
        setPaletteOpen(true);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // `?` (not while typing in a box or a terminal) or Ctrl+/ opens the keyboard shortcuts sheet.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing || e.altKey || e.defaultPrevented) return;
      const questionMark = e.key === '?' && !e.ctrlKey && !e.metaKey && !isTypingTarget(e.target) && !inTerminal(e.target);
      const ctrlSlash = e.key === '/' && (e.ctrlKey || e.metaKey) && !e.shiftKey;
      if (!questionMark && !ctrlSlash) return;
      e.preventDefault();
      setShortcutsOpen(true);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // Ctrl+\ (see toggleSplit). Re-listens each render so it always sees the current selection and split.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isSplitShortcut(e) || e.isComposing) return;
      // Ctrl+\ is the terminal's quit key (SIGQUIT): the split takes it, so the pane does not get it too.
      e.preventDefault();
      e.stopPropagation();
      toggleSplit();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });

  // Ctrl+Alt+Left / Right (Cmd+Alt on a Mac) move the focus between the two sides. Only while the split is open:
  // otherwise the keys go on to the terminal as before.
  useEffect(() => {
    if (split === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;
      const to = splitFocusKey(e, isMacPlatform());
      if (!to) return;
      e.preventDefault();
      e.stopPropagation();
      setSide(to);
      focusSideComposer(to);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [split]);

  // The split comes back on start when both of its panes still exist (split-memory.ts). The left pane is selected
  // again too: the app starts with no pane selected. Runs once, when the pane list is first known.
  const restoredSplit = useRef(false);
  useEffect(() => {
    if (!c.registry.initialReady || restoredSplit.current) return;
    restoredSplit.current = true;
    const memory = restorableSplit(readSplitMemory(), { width, hasPane: (id) => c.registry.panes.has(id) });
    if (!memory) return;
    nav.select(memory.left);
    setSplit(memory.right);
    setRatio(memory.ratio);
    setSide('left');
  }, [c.registry.initialReady]);
  // Saves the split while it is on. A split that ends (closed, the inbox picked, its pane gone, a narrow window)
  // clears the saved one, but only if this window saved one: a start without a restored split leaves storage alone.
  const savedSplit = useRef(false);
  useEffect(() => {
    if (split !== null && c.selectedId !== null) {
      writeSplitMemory({ left: c.selectedId, right: split, ratio });
      savedSplit.current = true;
    } else if (savedSplit.current) {
      clearSplitMemory();
      savedSplit.current = false;
    }
  }, [split, ratio, c.selectedId]);

  const { onClickCapture } = useNudgeOnAction();
  // Mission cards (Labs) count once in the one list, as they do in the Rust snapshot (tray, phone).
  const missionList = useMissions();
  const needsYou = useNeedsYouSource(c, loop.signals, undefined, missionList.missions, companions.companions ?? undefined);
  // Simple mode (Settings > General) swaps the whole face; off by default, so this line does nothing then.
  const simple = useSimpleMode();
  // While the welcome is open the normal face stays, so its sign-in and install windows are visible.
  if (simple && c.mode !== 'individual' && !welcome.open) {
    return (
      <>
        {/* No face hides the LIVE bar: in Simple mode it sits over the top edge. */}
        <div className="cockpit ck-live-top" data-theme={c.theme}>
          <LiveBar />
        </div>
        <SimpleCockpit c={c} loop={loop} needsYou={needsYou} lastTurn={away.lastTurn} />
      </>
    );
  }

  return (
    <LoopGuardContext.Provider value={loop}>
    <NeedsYouContext.Provider value={needsYou}>
    <LastTurnContext.Provider value={away.lastTurn}>
    <LiveStatusContext.Provider value={live.status}>
    <div className="cockpit" data-theme={c.theme} onClickCapture={onClickCapture}>
      <Sidebar
        c={nav}
        onOpenPalette={() => {
          setPaletteMode('commands');
          setPaletteOpen(true);
        }}
        onOpenDoctor={() => setView('doctor')}
        onOpenMemory={() => setView('memory')}
        onOpenPlans={() => setView('plans')}
        onOpenMorning={() => setView('morning')}
        onOpenScores={() => setView('scores')}
        onOpenStore={openStore}
        companions={companions.companions}
        openCompanionId={view === 'companion' ? companionId : null}
        onOpenCompanion={openCompanion}
        onNewCompanion={() => setSetupOpen(true)}
        onOpenToSide={openToSide}
      />
      <main className="ck-main">
        {c.mode === 'individual' ? (
          <div className="ck-spawn ck-spawn--plain">
            <h1 className="ck-title">Individual mode</h1>
            <p className="ck-sub">Panes are in their own windows.</p>
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void c.setMode('orchestration')}>
              Switch to Orchestration
            </button>
          </div>
        ) : view === 'doctor' ? (
          <AgentDoctor
            onOpenAgent={(id) => void c.newAgent(id)}
            onNotify={c.notify}
            onShowPane={(id) => nav.select(id)}
            onClose={closeOverlay}
            onOpenStore={openStore}
          />
        ) : view === 'store' ? (
          <StoreHost onOpenAgent={(id) => void c.newAgent(id)} onBack={closeOverlay} />
        ) : view === 'memory' ? (
          <ProjectMemory
            // A pane in a separate copy belongs to the project the copy came from, not to the copy's folder.
            project={c.selectedId ? projectOfCwd(c.registry.panes.get(c.selectedId)?.info.cwd)?.path : undefined}
            onClose={closeOverlay}
          />
        ) : view === 'plans' ? (
          <PlansMonth onClose={closeOverlay} />
        ) : view === 'morning' ? (
          <MorningPage onClose={closeOverlay} onOpenPane={(id) => nav.select(id)} />
        ) : view === 'scores' ? (
          <AgentScores
            // Same project rule as Project memory: a pane in a separate copy belongs to the project the copy came from.
            project={c.selectedId ? projectOfCwd(c.registry.panes.get(c.selectedId)?.info.cwd) : null}
            onClose={closeOverlay}
          />
        ) : view === 'settings' ? (
          <SettingsView key={settings.viewKey} {...settings.viewProps} onOpenStore={openStore} />
        ) : view === 'spawn' && c.spawn ? (
          <SpawnView spawn={c.spawn} onDismiss={c.dismissSpawn} />
        ) : view === 'companion' && companion ? (
          <CompanionPage
            key={companion.id}
            companion={companion}
            transport={tauriCompanionTransport}
            onShowTerminal={(paneId) => nav.select(paneId)}
            onOpenCard={() => nav.select(null)}
            onNotify={c.notify}
            onChanged={companions.refresh}
            onArchived={() => {
              setCompanionId(null);
              nav.select(null);
            }}
          />
        ) : view === 'pane' && c.selectedId && splitOn && split ? (
          <SplitView c={nav} leftId={c.selectedId} rightId={split} focus={side} ratio={ratio} onFocus={setSide} onRatio={setRatio} onClose={toggleSplit} />
        ) : view === 'pane' && c.selectedId ? (
          <PaneView c={c} paneId={c.selectedId} key={c.selectedId} />
        ) : (
          <Inbox
            c={nav}
            onOpenDoctor={() => setView('doctor')}
            onSetUp={welcomeSetUp}
            companions={companions.companions}
            onRestartCompanion={(id) => tauriCompanionTransport.restart(id).finally(companions.refresh)}
            companionTransport={{
              agents: () => tauriCompanionTransport.agents(),
              update: (id, patch) => tauriCompanionTransport.update(id, patch).finally(companions.refresh),
            }}
          />
        )}
        <LiveBar />
        {away.digest && c.mode !== 'individual' && (
          <AwayDigest
            digest={away.digest}
            now={c.now}
            onDismiss={away.dismissDigest}
            onOpenPane={(id) => {
              away.dismissDigest();
              nav.select(id);
            }}
            onOpenInbox={() => {
              away.dismissDigest();
              nav.select(null);
            }}
          />
        )}
      </main>
      {paletteOpen && (
        <Palette
          key={paletteMode}
          c={nav}
          initialMode={paletteMode}
          onClose={() => setPaletteOpen(false)}
          onOpenDoctor={() => setView('doctor')}
          onStopAll={stopAll}
          onOpenMemory={() => setView('memory')}
          onOpenPlans={() => setView('plans')}
          onOpenMorning={() => setView('morning')}
          onOpenScores={() => setView('scores')}
          onOpenStore={openStore}
          onUndoLastTurn={setUndoPane}
          onOpenWelcome={welcome.reopen}
          onStartCrew={() => setCrewOpen(true)}
          onOpenShortcuts={() => setShortcutsOpen(true)}
          companions={companions.companions}
          onOpenCompanion={openCompanion}
          onNewCompanion={companions.companions ? () => setSetupOpen(true) : undefined}
        />
      )}
      {crewOpen && (
        <StartCrewSheet
          defaultFolder={c.selectedId ? (c.registry.panes.get(c.selectedId)?.info.cwd ?? null) : null}
          agentName={(id) => agentLabel(agentListing, id)}
          onClose={() => setCrewOpen(false)}
        />
      )}
      {goLive && (
        <GoLiveSheet
          panes={sharePanes(c.registry.paneList, c.names, agentListing)}
          currentPaneId={goLive.paneId ?? c.selectedId ?? undefined}
          teamRelay={teamRelay}
          onClose={() => setGoLive(null)}
          onOpenSettings={() => {
            setGoLive(null);
            void nav.openSettings('general');
          }}
        />
      )}
      {shortcutsOpen && <ShortcutsSheet onClose={() => setShortcutsOpen(false)} />}
      {setupOpen && (
        <CompanionSetupSheet
          transport={tauriCompanionTransport}
          onClose={() => setSetupOpen(false)}
          onCreated={(created) => {
            setSetupOpen(false);
            companions.refresh();
            openCompanion(created.id);
          }}
        />
      )}
      {undoPane && (
        <UndoSheet
          paneId={undoPane}
          sharedWith={panesSharingFolder(c.registry.paneList, c.names, undoPane)}
          notify={c.notify}
          onClose={() => setUndoPane(null)}
        />
      )}
      {welcome.open && c.mode !== 'individual' && <FirstRun key={welcome.openKey} c={nav} seed={welcome.seed} onFinish={welcome.finish} />}
      {c.mode !== 'individual' && <TellTeamToast />}
      <ImportHost c={nav} />
      {stopReport && <PanicSheet report={stopReport} names={c.names} onClose={() => setStopReport(null)} />}
      <div className="ck-toasts" aria-live="polite">
        {c.toasts.map((t) => (
          <div className="ck-toast" key={t.id}>
            <span>{t.text}</span>
            {t.action && (
              <button type="button" className="ck-toast-action" onClick={t.action.run}>
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
    </LiveStatusContext.Provider>
    </LastTurnContext.Provider>
    </NeedsYouContext.Provider>
    </LoopGuardContext.Provider>
  );
}
