// Cockpit window (`index.html?cockpit=1`): one window that orchestrates every agent pane.
import { useEffect, useState } from 'react';
import { AgentDoctor } from './components/cockpit/AgentDoctor';
import { markSpawned } from './lib/agent-health';
import { useCockpit } from './hooks/useCockpit';
import { Inbox } from './components/cockpit/Inbox';
import { PaneView } from './components/cockpit/PaneView';
import { Palette } from './components/cockpit/Palette';
import { Sidebar } from './components/cockpit/Sidebar';
import { SpawnView } from './components/cockpit/SpawnView';
import './styles/cockpit.css';

export default function CockpitApp() {
  const c = useCockpit();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [doctorOpen, setDoctorOpen] = useState(false);
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

  return (
    <div className="cockpit" data-theme={c.theme}>
      <Sidebar c={nav} onOpenPalette={() => setPaletteOpen(true)} onOpenDoctor={() => setDoctorOpen(true)} />
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
        ) : c.spawn ? (
          <SpawnView spawn={c.spawn} onDismiss={c.dismissSpawn} />
        ) : c.selectedId ? (
          <PaneView c={c} paneId={c.selectedId} key={c.selectedId} />
        ) : (
          <Inbox c={nav} onOpenDoctor={() => setDoctorOpen(true)} />
        )}
      </main>
      {paletteOpen && <Palette c={nav} onClose={() => setPaletteOpen(false)} onOpenDoctor={() => setDoctorOpen(true)} />}
      <div className="ck-toasts" aria-live="polite">
        {c.toasts.map((t) => (
          <div className="ck-toast" key={t.id}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
