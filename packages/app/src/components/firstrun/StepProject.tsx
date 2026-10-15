// Step 3: the folder of the first project. Recent first, then the system picker. Every folder passes
// Simple mode's broad-folder guard (`plain_check_folder`, Rust) before it counts; a folder without Git
// is offered "Set up Git here" with the exact commands shown first, like the "+" menu.
import { useCallback, useEffect, useState } from 'react';
import type { GitInitPlan, RecentProject, ShipApi } from '../../lib/ship-api';
import { stateHint } from '../../lib/ship-model';
import type { SimpleApi } from '../simple/simple-api';
import { T } from './copy';
import type { ProjectPick } from './model';

type Ship = Pick<ShipApi, 'recent' | 'pickFolder' | 'forget' | 'gitInitPlan' | 'gitInit'>;

/** What each refusal from Rust says here. */
export function folderText(code: string | null): string {
  switch (code) {
    case 'FOLDER_TOO_BROAD':
      return T.tooBroad;
    case 'FOLDER_TOO_BIG':
      return T.tooBig;
    case 'FOLDER_MISSING':
      return T.missing;
    default:
      return T.failedFolder;
  }
}

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err)).replace(/^[A-Z_]+: /, '');

export function StepProject({
  project,
  ship,
  simple,
  onProject,
}: {
  project: ProjectPick | null;
  ship: Ship;
  simple: Pick<SimpleApi, 'checkFolder'>;
  onProject: (project: ProjectPick | null) => void;
}) {
  const [recent, setRecent] = useState<RecentProject[] | null>(null);
  /** Folders the guard refuses, by path (so a row says so before it is clicked). */
  const [broad, setBroad] = useState<Record<string, string>>({});
  const [problem, setProblem] = useState<string | null>(null);
  const [plan, setPlan] = useState<GitInitPlan | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    ship
      .recent()
      .then((list) => {
        setRecent(list);
        for (const p of list) {
          if (p.state === 'missing') continue;
          simple
            .checkFolder(p.path)
            .then((check) => !check.ok && setBroad((b) => ({ ...b, [p.path]: check.code })))
            .catch(() => undefined);
        }
      })
      .catch(() => setRecent([]));
  }, [ship, simple]);

  useEffect(load, [load]);

  // A folder chosen earlier (a restart) that lacks Git shows its plan again.
  useEffect(() => {
    if (project && project.state !== 'repo' && project.state !== 'missing') {
      ship.gitInitPlan(project.path).then(setPlan).catch(() => undefined);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const choose = async (found: RecentProject) => {
    setProblem(null);
    setPlan(null);
    if (found.state === 'missing') return;
    try {
      const check = await simple.checkFolder(found.path);
      if (!check.ok) {
        onProject(null);
        setProblem(folderText(check.code));
        return;
      }
    } catch {
      onProject(null);
      setProblem(T.failedFolder);
      return;
    }
    onProject({ path: found.path, name: found.name, state: found.state });
    if (found.state !== 'repo') {
      ship.gitInitPlan(found.path).then(setPlan).catch((err) => setProblem(errText(err)));
    }
  };

  const pick = async () => {
    setProblem(null);
    try {
      const found = await ship.pickFolder();
      if (found) {
        load();
        await choose(found);
      }
    } catch (err) {
      setProblem(errText(err));
    }
  };

  const setUpGit = async () => {
    if (!project) return;
    setBusy(true);
    setProblem(null);
    try {
      await ship.gitInit(project.path);
      onProject({ ...project, state: 'repo' });
      setPlan(null);
      load();
    } catch (err) {
      setProblem(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const forget = (found: RecentProject) => {
    ship
      .forget(found.path)
      .catch(() => undefined)
      .finally(load);
  };

  return (
    <section className="fr-step" aria-labelledby="fr-project-t">
      <div className="ck-doto fr-num" aria-hidden="true">03</div>
      <h1 className="ck-title" id="fr-project-t">{T.projectTitle}</h1>
      <p className="fr-lead">{T.projectLead}</p>

      {project && (
        <div className="fr-picked" data-testid="picked-project">
          <span className="fr-picked-name">{project.name}</span>
          <span className="fr-path" title={project.path}>{project.path}</span>
          {project.state === 'repo' && <span className="fr-picked-ok">{T.projectChosen}</span>}
        </div>
      )}

      {project && project.state !== 'repo' && project.state !== 'missing' && (
        <div className="fr-git" data-testid="git-step">
          <h2 className="ck-section">{T.needsGitTitle}</h2>
          <p className="fr-hint">{T.needsGitBody}</p>
          {plan ? (
            <>
              <ul className="fr-cmds" aria-label="What will run">
                {plan.commands.map((cmd) => (
                  <li key={cmd}>
                    <code>{cmd}</code>
                  </li>
                ))}
              </ul>
              <p className="fr-hint">{plan.too_big ? T.gitTooBig : T.gitFiles(plan.files)}</p>
            </>
          ) : (
            !problem && <p className="fr-hint">{T.checking}</p>
          )}
          <div className="fr-actions">
            <button type="button" className="ck-btn ck-btn--primary" disabled={!plan || plan.too_big || busy} onClick={() => void setUpGit()}>
              {busy ? T.gitWorking : T.setUpGit}
            </button>
          </div>
        </div>
      )}

      {problem && (
        <p className="fr-problem" role="alert">
          {problem}
        </p>
      )}

      <h2 className="fr-cap">{T.recent}</h2>
      {recent === null && <p className="fr-hint">{T.checking}</p>}
      {recent?.length === 0 && <p className="fr-hint">{T.noRecent}</p>}
      <ul className="fr-list" aria-label={T.recent}>
        {recent?.map((p) => {
          const refused = broad[p.path];
          const off = p.state === 'missing' || Boolean(refused);
          return (
            <li key={p.path} className="fr-list-row">
              <button
                type="button"
                className="fr-row"
                data-selected={project?.path === p.path ? 'true' : undefined}
                disabled={off}
                onClick={() => void choose(p)}
                title={p.path}
              >
                <span className="fr-row-main">
                  <span className="fr-row-name">{p.name}</span>
                  <span className="fr-path">{p.path}</span>
                </span>
                {(refused || stateHint(p.state)) && <span className="fr-row-hint">{refused ? folderText(refused).split('.')[0] : stateHint(p.state)}</span>}
              </button>
              {p.state === 'missing' && (
                <button type="button" className="ck-btn ck-btn--plain ck-btn--sm" aria-label={`${T.remove} ${p.name}`} onClick={() => forget(p)}>
                  {T.remove}
                </button>
              )}
            </li>
          );
        })}
      </ul>
      <div className="fr-actions">
        <button type="button" className="ck-btn ck-btn--secondary" onClick={() => void pick()}>
          {T.chooseFolder}
        </button>
      </div>
    </section>
  );
}
