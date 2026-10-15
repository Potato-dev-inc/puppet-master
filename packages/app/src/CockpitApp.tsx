// Cockpit window (`index.html?cockpit=1`): one window that orchestrates every agent pane.
import { useNudgeOnAction } from './lib/desktop-nudge';
import { useEffect, useState } from 'react';
import { AgentDoctor } from './components/cockpit/AgentDoctor';
import { markSpawned } from './lib/agent-health';
import { useCockpit } from './hooks/useCockpit';
import { useAway } from './hooks/useAway';
import { LastTurnContext } from './components/cockpit/away-context';
import { AwayDigest } from './components/cockpit/AwayDigest';
import { Inbox } from './components/cockpit/Inbox';
import { PaneView } from './components/cockpit/PaneView';
import { Palette } from './components/cockpit/Palette';
import { NeedsYouContext, useNeedsYouSource } from './hooks/useNeedsYou';
import { UndoSheet } from './components/cockpit/UndoSheet';
import { panesSharingFolder } from './components/cockpit/history-model';
import { ProjectMemory } from './components/cockpit/ProjectMemory';
import { Sidebar } from './components/cockpit/Sidebar';
import { SettingsView } from './components/cockpit/SettingsView';
import { useInlineSettings } from './hooks/useInlineSettings';
import { SpawnView } from './components/cockpit/SpawnView';
import { PanicSheet } from './components/cockpit/PanicSheet';
import { TellTeamToast } from './components/cockpit/team/TellTeam';
import { SimpleCockpit } from './components/simple/SimpleCockpit';
import { useSimpleMode } from './lib/simple-mode';
import { projectOfCwd } from './lib/ship-model';
import { LoopGuardContext, tauriLoopApi, useLoopSignals, type StopReport } from './lib/loop-guard';
import { homeView, viewAfterSelect, type MainView } from './components/cockpit/main-view';
import './styles/cockpit.css';

export type { MainView } from './components/cockpit/main-view';

export default function CockpitApp() {
  const c = useCockpit();
  const [paletteOpen, setPaletteOpen] = useState(false);
  // The one thing the main area shows (see main-view.ts): the last click wins.
  const [view, setView] = useState<MainView>(() => homeView(c.spawn !== null, c.selectedId));
  // A new selection or spawn state (a pane closing, an agent starting, a digest opening a pane)
  // returns to the resting view. Done while rendering, so no frame shows a stale view.
  const [seen, setSeen] = useState({ spawn: c.spawn, selectedId: c.selectedId });
  if (seen.spawn !== c.spawn || seen.selectedId !== c.selectedId) {
    setSeen({ spawn: c.spawn, selectedId: c.selectedId });
    setView(homeView(c.spawn !== null, c.selectedId));
  }
  const closeOverlay = () => setView(homeView(c.spawn !== null, c.selectedId));
  // Settings is a main view in Orchestration mode; in Individual mode `open` asks for the Settings window.
  const settings = useInlineSettings<MainView>({ mode: c.mode, view, setView, home: homeView(c.spawn !== null, c.selectedId), openWindow: c.openSettings });
  // Pane whose last turn the person is being asked to undo (opened from the palette).
  const [undoPane, setUndoPane] = useState<string | null>(null);
  const loop = useLoopSignals();
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
      c.select(id);
    },
  };
  useEffect(() => {
    if (c.registry.paneList.length > 0) markSpawned();
  }, [c.registry.paneList.length]);
  const away = useAway(c.registry.paneList.map((p) => p.info.id));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  const { onClickCapture } = useNudgeOnAction();
  const needsYou = useNeedsYouSource(c, loop.signals);
  // Simple mode (Settings > General) swaps the whole face; off by default, so this line does nothing then.
  const simple = useSimpleMode();
  if (simple && c.mode !== 'individual') return <SimpleCockpit c={c} loop={loop} needsYou={needsYou} lastTurn={away.lastTurn} />;

  return (
    <LoopGuardContext.Provider value={loop}>
    <NeedsYouContext.Provider value={needsYou}>
    <LastTurnContext.Provider value={away.lastTurn}>
    <div className="cockpit" data-theme={c.theme} onClickCapture={onClickCapture}>
      <Sidebar
        c={nav}
        onOpenPalette={() => setPaletteOpen(true)}
        onOpenDoctor={() => setView('doctor')}
        onOpenMemory={() => setView('memory')}
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
          />
        ) : view === 'memory' ? (
          <ProjectMemory
            // A pane in a separate copy belongs to the project the copy came from, not to the copy's folder.
            project={c.selectedId ? projectOfCwd(c.registry.panes.get(c.selectedId)?.info.cwd)?.path : undefined}
            onClose={closeOverlay}
          />
        ) : view === 'settings' ? (
          <SettingsView key={settings.viewKey} {...settings.viewProps} />
        ) : view === 'spawn' && c.spawn ? (
          <SpawnView spawn={c.spawn} onDismiss={c.dismissSpawn} />
        ) : view === 'pane' && c.selectedId ? (
          <PaneView c={c} paneId={c.selectedId} key={c.selectedId} />
        ) : (
          <Inbox c={nav} onOpenDoctor={() => setView('doctor')} />
        )}
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
          c={nav}
          onClose={() => setPaletteOpen(false)}
          onOpenDoctor={() => setView('doctor')}
          onStopAll={stopAll}
          onOpenMemory={() => setView('memory')}
          onUndoLastTurn={setUndoPane}
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
      {c.mode !== 'individual' && <TellTeamToast />}
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
    </LastTurnContext.Provider>
    </NeedsYouContext.Provider>
    </LoopGuardContext.Provider>
  );
}
