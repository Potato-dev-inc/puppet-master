// Simple mode as a whole window: the same cockpit data (`useCockpit`), a different face. This is
// the only entry point; CockpitApp renders it in one line when the switch is on and not otherwise.
//
// Screens: Home (her projects), Make something, and one project. The project being looked at is
// the cockpit's own selection, so a pane that closes (Keep, Throw away) lands back on Home.
import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../hooks/useCockpit';
import { NeedsYouContext } from '../../hooks/useNeedsYou';
import type { NeedsYouItem } from '../../lib/attention';
import { LoopGuardContext, type LoopGuardState } from '../../lib/loop-guard';
import { LastTurnContext } from '../cockpit/away-context';
import type { TurnSummary } from '../../lib/cockpit-api';
import '../../styles/simple.css';
import { MakeSomething } from './MakeSomething';
import { normalize } from './folder-guard';
import { SimpleHome, useCopyMap } from './SimpleHome';
import { SimpleProject } from './SimpleProject';
import { useCopy } from './use-copy';
import { HelpersScreen } from './HelpersScreen';
import { PaneView } from '../cockpit/PaneView';
import { patchSettings } from '../../lib/settings';
import { T } from './copy';

type Screen = 'home' | 'make' | 'helpers';


function Project({ c, paneId, onBack }: { c: CockpitApi; paneId: string; onBack: () => void }) {
  return (
    <SimpleProject
      c={c}
      paneId={paneId}
      key={paneId}
      onBack={onBack}
      onStartedAgain={(id) => {
        if (id) c.select(id);
      }}
    />
  );
}

export function SimpleCockpit({
  c,
  loop,
  needsYou,
  lastTurn,
}: {
  c: CockpitApi;
  loop: LoopGuardState;
  needsYou: NeedsYouItem[];
  lastTurn: ReadonlyMap<string, TurnSummary>;
}) {
  const [screen, setScreen] = useState<Screen>('home');
  // A project that was just started opens as soon as its pane is listed. The start answer may come
  // before the pane exists, so the project is also found by its folder.
  const [opening, setOpening] = useState<{ paneId: string | null; folder: string } | null>(null);
  const copies = useCopyMap(c);
  useEffect(() => {
    if (!opening) return;
    const byFolder = [...copies.entries()].find(([, item]) => normalize(item.repo) === normalize(opening.folder))?.[0];
    const id = opening.paneId && c.registry.panes.has(opening.paneId) ? opening.paneId : byFolder;
    if (id && c.registry.panes.has(id)) {
      setOpening(null);
      c.select(id);
    }
  }, [opening, copies, c]);
  const selected = c.selectedId && c.registry.panes.has(c.selectedId) ? c.selectedId : null;
  // Look the copy up here as well, so a pane that is not on a copy never opens as a Simple project.
  const status = selected ? (c.registry.panes.get(selected)?.status ?? '') : '';
  const copy = useCopy(selected ?? '', status);
  const showProject = selected !== null && (!copy.loaded || copy.item !== null);
  // A window that is not a project (a sign-in or an install, from Set up agents) shows as it is.
  const showWindow = selected !== null && copy.loaded && copy.item === null;

  return (
    <LoopGuardContext.Provider value={loop}>
      <NeedsYouContext.Provider value={needsYou}>
        <LastTurnContext.Provider value={lastTurn}>
          <div className="cockpit sx" data-theme={c.theme} data-simple="on">
            <main className="sx-main">
              {showProject && selected ? (
                <Project c={c} paneId={selected} onBack={() => c.select(null)} />
              ) : showWindow && selected ? (
                <div className="sx-window">
                  <button type="button" className="sx-back" onClick={() => c.select(null)}>
                    ‹ {T.helpersBack}
                  </button>
                  <PaneView c={c} paneId={selected} key={selected} />
                </div>
              ) : screen === 'helpers' ? (
                <HelpersScreen c={c} onClose={() => setScreen('home')} />
              ) : screen === 'make' ? (
                <MakeSomething
                  onCancel={() => setScreen('home')}
                  onOpenSettings={() => setScreen('helpers')}
                  onStarted={(started) => {
                    setScreen('home');
                    setOpening({ paneId: started.pane_id, folder: started.folder });
                  }}
                />
              ) : (
                <SimpleHome
                  c={c}
                  onMake={() => setScreen('make')}
                  onOpen={(id) => c.select(id)}
                  onHelpers={() => setScreen('helpers')}
                  onTurnOff={() => void patchSettings({ simple_mode: false })}
                />
              )}
            </main>
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
