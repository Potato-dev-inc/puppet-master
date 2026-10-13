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
import { SpawnView } from './components/cockpit/SpawnView';
import { PanicSheet } from './components/cockpit/PanicSheet';
import { LoopGuardContext, tauriLoopApi, useLoopSignals, type StopReport } from './lib/loop-guard';
import './styles/cockpit.css';

export default function CockpitApp() {
  const c = useCockpit();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [doctorOpen, setDoctorOpen] = useState(false);
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
  // Any navigation (a pane, a spawn) leaves the "Set up agents" view.
  const nav: typeof c = {
    ...c,
    select: (id) => {
      setDoctorOpen(false);
      c.select(id);
    },
  };
  useEffect(() => {
    if (c.spawn) setDoctorOpen(false);
  }, [c.spawn]);
  useEffect(() => {
    if (c.registry.paneList.length > 0) markSpawned();
  }, [c.registry.paneList.length]);
  const [memoryOpen, setMemoryOpen] = useState(false);
  // Picking a pane or starting an agent leaves the memory view.
  useEffect(() => setMemoryOpen(false), [c.selectedId, c.spawn]);
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

  return (
    <LoopGuardContext.Provider value={loop}>
    <NeedsYouContext.Provider value={needsYou}>
    <LastTurnContext.Provider value={away.lastTurn}>
    <div className="cockpit" data-theme={c.theme} onClickCapture={onClickCapture}>
      <Sidebar
        c={nav}
        onOpenPalette={() => setPaletteOpen(true)}
        onOpenDoctor={() => setDoctorOpen(true)}
        onOpenMemory={() => setMemoryOpen(true)}
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
        ) : doctorOpen ? (
          <AgentDoctor
            onOpenAgent={(id) => void c.newAgent(id)}
            onNotify={c.notify}
            onClose={() => setDoctorOpen(false)}
          />
        ) : memoryOpen ? (
          <ProjectMemory
            project={c.selectedId ? c.registry.panes.get(c.selectedId)?.info.cwd : undefined}
            onClose={() => setMemoryOpen(false)}
          />
        ) : c.spawn ? (
          <SpawnView spawn={c.spawn} onDismiss={c.dismissSpawn} />
        ) : c.selectedId ? (
          <PaneView c={c} paneId={c.selectedId} key={c.selectedId} />
        ) : (
          <Inbox c={nav} onOpenDoctor={() => setDoctorOpen(true)} />
        )}
        {away.digest && c.mode !== 'individual' && (
          <AwayDigest
            digest={away.digest}
            now={c.now}
            onDismiss={away.dismissDigest}
            onOpenPane={(id) => {
              away.dismissDigest();
              c.select(id);
            }}
            onOpenInbox={() => {
              away.dismissDigest();
              c.select(null);
            }}
          />
        )}
      </main>
      {paletteOpen && (
        <Palette
          c={nav}
          onClose={() => setPaletteOpen(false)}
          onOpenDoctor={() => setDoctorOpen(true)}
          onStopAll={stopAll}
          onOpenMemory={() => setMemoryOpen(true)}
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
