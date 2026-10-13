// Cockpit window (`index.html?cockpit=1`): one window that orchestrates every agent pane.
import { useNudgeOnAction } from './lib/desktop-nudge';
import { useEffect, useState } from 'react';
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

  return (
    <div className="cockpit" data-theme={c.theme} onClickCapture={onClickCapture}>
      <Sidebar c={c} onOpenPalette={() => setPaletteOpen(true)} />
      <main className="ck-main">
        {c.mode === 'individual' ? (
          <div className="ck-spawn ck-spawn--plain">
            <h1 className="ck-title">Individual mode</h1>
            <p className="ck-sub">Panes are in their own windows.</p>
            <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void c.setMode('orchestration')}>
              Switch to Orchestration
            </button>
          </div>
        ) : c.spawn ? (
          <SpawnView spawn={c.spawn} onDismiss={c.dismissSpawn} />
        ) : c.selectedId ? (
          <PaneView c={c} paneId={c.selectedId} key={c.selectedId} />
        ) : (
          <Inbox c={c} />
        )}
      </main>
      {paletteOpen && <Palette c={c} onClose={() => setPaletteOpen(false)} />}
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
