// Where the Tonight sheet gets its folders: the recent projects plus the folders of the open panes (a pane in a
// separate copy counts as the project it came from). A folder is "git" when a separate copy can be made of it.
// The sheet is drawn once the list is known, so its first folder pick is never empty by a race.
import { useEffect, useState } from 'react';
import type { CockpitApi } from '../../../hooks/useCockpit';
import { useUndoOn } from '../../../lib/night-host';
import type { NightView } from '../../../lib/night';
import { shipApi, type GitState, type ShipApi } from '../../../lib/ship-api';
import { projectOfCwd, realRecents } from '../../../lib/ship-model';
import { TonightSheet, type TonightProject } from './TonightSheet';

/** Pure: the folders to offer, from the recents and the pane folders; `states` holds a pane folder's Git state when known. */
export function tonightProjects(
  recents: { path: string; name: string; state: GitState }[],
  paneCwds: (string | undefined)[],
  states: Record<string, GitState> = {},
): TonightProject[] {
  const out: TonightProject[] = [];
  const seen = new Set<string>();
  const add = (path: string, name: string, state: GitState | undefined) => {
    const key = path.toLowerCase();
    if (seen.has(key) || state === 'missing') return;
    seen.add(key);
    out.push({ path, name, git: state === 'repo' });
  };
  for (const cwd of paneCwds) {
    const p = projectOfCwd(cwd);
    if (!p) continue;
    const known = recents.find((r) => r.path.toLowerCase() === p.path.toLowerCase());
    add(p.path, p.name, known?.state ?? states[p.path]);
  }
  for (const r of recents) add(r.path, r.name, r.state);
  return out;
}

export function TonightHost({
  c,
  onClose,
  onStarted,
  ship = shipApi,
}: {
  c: Pick<CockpitApi, 'registry' | 'notify'>;
  onClose: () => void;
  onStarted?: (run: NightView) => void;
  ship?: ShipApi;
}) {
  const undoOn = useUndoOn();
  const [projects, setProjects] = useState<TonightProject[] | null>(null);
  useEffect(() => {
    let live = true;
    const cwds = c.registry.paneList.map((p) => p.info.cwd);
    (async () => {
      const recents = await ship.recent(20).then(realRecents).catch(() => []);
      const states: Record<string, GitState> = {};
      for (const cwd of cwds) {
        const p = projectOfCwd(cwd);
        if (!p || states[p.path] || recents.some((r) => r.path.toLowerCase() === p.path.toLowerCase())) continue;
        states[p.path] = await ship.gitState(p.path).catch(() => 'folder' as GitState);
      }
      if (live) setProjects(tonightProjects(recents, cwds, states));
    })();
    return () => {
      live = false;
    };
    // The list is read once, when the sheet opens.
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (!projects) return null;
  return (
    <TonightSheet
      projects={projects}
      undoOn={undoOn}
      onClose={onClose}
      onStarted={onStarted}
      onNotify={c.notify}
    />
  );
}
