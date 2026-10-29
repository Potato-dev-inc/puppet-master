// Body of the cockpit "+" menu. Three steps share one small panel:
//   1. pick an agent (an agent with user accounts gets a second step for the login);
//   2. the folder step: a project chip at the top opens Recent projects and "Choose folder…";
//   3. "Set up Git here" for a folder without Git, with the exact commands shown first.
// The Separate copy switch sits at the bottom of step 1. It starts OFF (the agent works in the folder the person
// selected); a remembered choice wins. Without Git it cannot be turned on: a one-line hint and "Set up Git here".
// The first row of step 1 is "Pick for me" (only with a `pick` API and a project). It asks which agent to start
// here, from the person's own history. It starts nothing: only a press on its Start button does.
import { useEffect, useState } from 'react';
import { accountsFor, needsAccountStep, tauriAccountsApi, type Account, type AccountsApi } from '../../lib/accounts';
import { shipApi, type GitInitPlan, type GitState, type RecentProject, type ShipApi } from '../../lib/ship-api';
import { realRecents, stateHint, supportsCopies } from '../../lib/ship-model';
import { EARLY_TITLE, FOOTER, isEarly, pickHint, startable, startLabel, type PickApi, type PickResult, type PickState } from '../../lib/pick-for-me';
import '../../styles/cockpit-ship.css';
import '../../styles/cockpit-pick.css';

export interface LaunchEntry {
  type: string;
  label: string;
  disabled?: boolean;
  disabledReason?: string;
}

/** The folder a new agent will work in. */
export interface ProjectChoice {
  path: string;
  name: string;
  /** Git state when known; `undefined` means "assume it works" (the selected pane's folder). */
  state?: RecentProject['state'];
}

interface Props {
  entries: LaunchEntry[];
  readyAgents: Set<string>;
  onStart: (agentId: string, accountId?: string) => void;
  /** Called after an agent was started, so the host closes the menu. */
  onClose: () => void;
  accountsApi?: AccountsApi;
  /** The folder step. Without `onProject` the menu has no project chip (the old, plain menu). */
  project?: ProjectChoice | null;
  onProject?: (project: ProjectChoice) => void;
  /** The Separate copy switch. Without `onIsolated` it is not drawn. */
  isolated?: boolean;
  onIsolated?: (on: boolean) => void;
  ship?: ShipApi;
  /** "Pick for me". Without it, or without `project`, the row is not drawn. */
  pick?: PickApi;
  /** "More agents…": opens the Agent Store. Without it the row is not drawn. */
  onOpenStore?: () => void;
  /** "Start a mission…" (Preview, Labs). Without it the item is not drawn. */
  onStartMission?: () => void;
  /** "Tonight…" (Night Shift, Labs). Without it the item is not drawn. */
  onTonight?: () => void;
}

type Step = 'agents' | 'projects' | 'git' | 'pick';

/** The answer of "Pick for me" for one folder. `path` is the folder it was asked for, so an old answer is never shown. */
interface PickAnswer {
  path: string;
  result?: PickResult;
  failed?: boolean;
}

/** The one line under a switch that cannot be turned on: why, in plain words. */
function noGitHint(state: GitState | undefined): string {
  if (state === 'no_commits') return 'Needs a first Git snapshot.';
  if (state === 'missing') return 'This folder was not found.';
  return 'Needs Git. This folder doesn’t use it yet.';
}

const errText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

export function NewAgentMenu({
  entries,
  readyAgents,
  onStart,
  onClose,
  accountsApi = tauriAccountsApi,
  project,
  onProject,
  isolated = false,
  onIsolated,
  ship = shipApi,
  pick: pickApi,
  onOpenStore,
  onStartMission,
  onTonight,
}: Props) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [picking, setPicking] = useState<LaunchEntry | null>(null);
  const [step, setStep] = useState<Step>('agents');
  const [recent, setRecent] = useState<RecentProject[] | null>(null);
  const [plan, setPlan] = useState<GitInitPlan | null>(null);
  const [planFor, setPlanFor] = useState<RecentProject | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [answer, setAnswer] = useState<PickAnswer | null>(null);
  const projectPath = project?.path ?? null;

  useEffect(() => {
    let alive = true;
    accountsApi
      .list()
      .then((o) => alive && setAccounts(o.accounts))
      .catch(() => undefined); // no accounts store (browser, old backend): the plain menu still works
    return () => {
      alive = false;
    };
  }, [accountsApi]);

  const loadRecent = () => {
    ship
      .recent()
      .then((list) => setRecent(realRecents(list)))
      .catch(() => setRecent([]));
  };

  useEffect(() => {
    if (onProject) loadRecent();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onProject]);

  // "Pick for me": asked once when the menu opens and again when the folder changes. The `alive` flag drops an
  // answer that arrives after the folder changed, so an old folder's pick is never shown. Nothing is started here.
  useEffect(() => {
    if (!pickApi || !projectPath) return undefined;
    let alive = true;
    setAnswer({ path: projectPath });
    pickApi
      .pick(projectPath)
      .then((result) => alive && setAnswer({ path: projectPath, result }))
      .catch(() => alive && setAnswer({ path: projectPath, failed: true }));
    return () => {
      alive = false;
    };
  }, [pickApi, projectPath]);

  // The pick state for the current folder only: 'loading', the answer, or 'failed' (then no row is drawn).
  const current = answer && answer.path === projectPath ? answer : null;
  let pickState: PickState | 'failed' | null = null;
  if (pickApi && projectPath) pickState = current?.result ?? (current?.failed ? 'failed' : 'loading');
  const pickResult = pickState !== null && pickState !== 'loading' && pickState !== 'failed' ? pickState : null;

  const start = (agentId: string, accountId?: string) => {
    onClose();
    onStart(agentId, accountId);
  };

  const choose = (found: RecentProject) => {
    setError(null);
    if (found.state === 'missing') return;
    if (found.state === 'repo') {
      onProject?.({ path: found.path, name: found.name, state: found.state });
      setStep('agents');
      return;
    }
    // No Git (or no first snapshot): show what "Set up Git here" would do before it does anything.
    setPlanFor(found);
    setPlan(null);
    setStep('git');
    ship
      .gitInitPlan(found.path)
      .then(setPlan)
      .catch((err) => setError(errText(err)));
  };

  const pick = async () => {
    setError(null);
    try {
      const found = await ship.pickFolder();
      if (found) {
        loadRecent();
        choose(found);
      }
    } catch (err) {
      setError(errText(err));
    }
  };

  const setUpGit = async () => {
    if (!planFor) return;
    setBusy(true);
    setError(null);
    try {
      await ship.gitInit(planFor.path);
      onProject?.({ path: planFor.path, name: planFor.name, state: 'repo' });
      setStep('agents');
      loadRecent();
    } catch (err) {
      setError(errText(err));
    } finally {
      setBusy(false);
    }
  };

  const withoutGit = () => {
    if (!planFor) return;
    onProject?.({ path: planFor.path, name: planFor.name, state: planFor.state });
    setStep('agents');
  };

  const forget = (found: RecentProject) => {
    ship
      .forget(found.path)
      .catch(() => undefined)
      .finally(loadRecent);
  };

  if (step === 'projects') {
    return (
      <>
        <div className="ck-menu-cap">Recent projects</div>
        {recent === null && <div className="ck-menu-note">Looking…</div>}
        {recent?.length === 0 && <div className="ck-menu-note">No projects yet. Choose a folder to start.</div>}
        {recent?.map((p) => (
          <div key={p.path} className="ck-menu-project">
            <button
              type="button"
              role="menuitem"
              className="ck-menu-item ck-menu-item--project"
              disabled={p.state === 'missing'}
              onClick={() => choose(p)}
              title={p.path}
            >
              <span className="ck-menu-project-main">
                <span className={p.state === 'missing' ? 'ck-menu-project-name is-dim' : 'ck-menu-project-name'}>{p.name}</span>
                <span className="ck-menu-project-path">{p.path}</span>
              </span>
              {stateHint(p.state) && <span className="ck-menu-hint">{stateHint(p.state)}</span>}
            </button>
            {p.state === 'missing' && (
              <button type="button" className="ck-menu-remove" aria-label={`Remove ${p.name} from the list`} onClick={() => forget(p)}>
                Remove
              </button>
            )}
          </div>
        ))}
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => void pick()}>
          <span>Choose folder…</span>
        </button>
        {error && (
          <div className="ck-menu-note ck-menu-note--err" role="alert">
            {error}
          </div>
        )}
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => setStep('agents')}>
          <span>Back</span>
        </button>
      </>
    );
  }

  if (step === 'git' && planFor) {
    return (
      <div className="ck-gitstep">
        <div className="ck-menu-cap">{planFor.name}</div>
        <p className="ck-gitstep-text">This folder doesn&apos;t use Git yet. Undo and separate copies need it.</p>
        {plan ? (
          <>
            <p className="ck-gitstep-fine">
              {plan.too_big
                ? 'This folder has too many files to save as one snapshot. Pick the project’s own folder instead.'
                : `Saves ${plan.files} ${plan.files === 1 ? 'file' : 'files'} as the first snapshot.`}
              {plan.gitignore.length > 0 && !plan.too_big && ' Also adds a .gitignore that keeps node_modules and .env files out.'}{' '}
              Keeps a private history of changes in this folder. Nothing is uploaded.
            </p>
            <details className="ck-gitstep-details">
              <summary>Details</summary>
              <ul className="ck-gitstep-cmds" aria-label="What will run">
                {plan.commands.map((c) => (
                  <li key={c}>
                    <code>{c}</code>
                  </li>
                ))}
              </ul>
            </details>
          </>
        ) : (
          !error && <p className="ck-gitstep-fine">Checking the folder…</p>
        )}
        {error && (
          <p className="ck-menu-note ck-menu-note--err" role="alert">
            {error}
          </p>
        )}
        <div className="ck-gitstep-actions">
          <button type="button" className="ck-btn ck-btn--primary" disabled={!plan || plan.too_big || busy} onClick={() => void setUpGit()}>
            Set up Git here
          </button>
          <button type="button" className="ck-btn ck-btn--secondary" disabled={busy} onClick={withoutGit}>
            Not now
          </button>
        </div>
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => setStep('projects')}>
          <span>Back</span>
        </button>
      </div>
    );
  }

  if (step === 'pick' && pickResult) {
    // The pick panel: the name, one plain sentence, Start (only when it can really start), and "Why this one".
    const { choice } = pickResult;
    const canStart = startable(pickResult, entries);
    return (
      <div className="ck-pick">
        <div className="ck-menu-cap">Pick for me</div>
        {choice && <div className="ck-pick-name">{choice.label}</div>}
        {choice && isEarly(pickResult) && (
          <span className="ck-pick-chip" title={EARLY_TITLE}>
            early
          </span>
        )}
        <p className="ck-pick-reason">{pickResult.reason}</p>
        {choice && !canStart && (
          <p className="ck-menu-note ck-menu-note--err" role="alert">
            {`${choice.label} can't be started from this menu.`}
          </p>
        )}
        <div className="ck-pick-actions">
          {choice && canStart ? (
            <>
              <button type="button" className="ck-btn ck-btn--primary" autoFocus onClick={() => start(choice.agentId, choice.accountId)}>
                {startLabel(pickResult)}
              </button>
              <button type="button" className="ck-btn ck-btn--secondary" onClick={() => setStep('agents')}>
                Choose myself
              </button>
            </>
          ) : (
            <button type="button" className="ck-btn ck-btn--secondary" autoFocus onClick={() => setStep('agents')}>
              Back
            </button>
          )}
        </div>
        <details className="ck-pick-why">
          <summary>Why this one</summary>
          {pickResult.considered.map((c, i) => (
            <div key={`${c.label}-${i}`} className={c.state === 'skipped' ? 'ck-pick-line is-skipped' : 'ck-pick-line'}>
              <span>{c.label}</span>
              <span>{c.line}</span>
            </div>
          ))}
          {pickResult.notes.map((note) => (
            <p key={note} className="ck-pick-foot">
              {note}
            </p>
          ))}
          <p className="ck-pick-foot">{FOOTER}</p>
        </details>
      </div>
    );
  }

  if (picking) {
    return (
      <>
        <div className="ck-menu-cap">{picking.label} · login</div>
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => start(picking.type)}>
          <span>Default login</span>
        </button>
        {accountsFor(accounts, picking.type).map((a) => (
          <button key={a.id} type="button" role="menuitem" className="ck-menu-item" onClick={() => start(picking.type, a.id)}>
            <span>{a.label}</span>
          </button>
        ))}
        <button type="button" role="menuitem" className="ck-menu-item" onClick={() => setPicking(null)}>
          <span>Back</span>
        </button>
      </>
    );
  }

  const copiesOk = supportsCopies(project?.state);
  const canSetUpGit = project?.state === 'folder' || project?.state === 'no_commits';
  return (
    <>
      <div className="ck-menu-cap ck-menu-cap--row">
        <span>New agent</span>
        {onProject && (
          <button
            type="button"
            className="ck-chip"
            aria-label={`Project: ${project?.name ?? 'current'}. Change`}
            aria-haspopup="menu"
            onClick={() => {
              loadRecent();
              setStep('projects');
            }}
          >
            <span className="ck-chip-name">{project?.name ?? 'Current project'}</span>
            <span aria-hidden="true">▾</span>
          </button>
        )}
      </div>
      {pickState !== null && pickState !== 'failed' && (
        <>
          <button type="button" role="menuitem" className="ck-menu-item" disabled={pickState === 'loading'} onClick={() => setStep('pick')}>
            <span>Pick for me</span>
            <span className="ck-menu-hint">{pickHint(pickState)}</span>
          </button>
          <div className="ck-menu-sep" />
        </>
      )}
      {entries.map((e) => {
        const hasAccounts = needsAccountStep(accounts, e.type);
        return (
          <button
            key={e.type}
            type="button"
            role="menuitem"
            className="ck-menu-item"
            disabled={e.disabled}
            title={e.disabledReason}
            onClick={() => (hasAccounts ? setPicking(e) : start(e.type))}
          >
            <span>{e.label}</span>
            {!e.disabled && hasAccounts && <span className="ck-menu-hint">{accountsFor(accounts, e.type).length + 1} logins ›</span>}
            {!e.disabled && !hasAccounts && readyAgents.has(e.type) && <span className="ck-menu-hint">ready</span>}
            {e.disabled && <span className="ck-menu-hint ck-menu-hint--long">only for tasks from other agents</span>}
          </button>
        );
      })}
      {onIsolated && (
        <>
          <div className="ck-menu-toggle" title="Work in a separate copy of the project, so agents never edit the same files. You review and merge it later.">
            <span id="ck-sepcopy-label">Separate copy</span>
            <button
              type="button"
              role="switch"
              aria-checked={isolated && copiesOk}
              aria-disabled={!copiesOk}
              aria-labelledby="ck-sepcopy-label"
              className="ck-switch"
              disabled={!copiesOk}
              onClick={() => onIsolated(!isolated)}
            >
              <span className="ck-switch-knob" />
            </button>
          </div>
          {!copiesOk ? (
            <div className="ck-menu-note ck-menu-note--row">
              <span>{noGitHint(project?.state)}</span>
              {project && canSetUpGit && (
                <button type="button" className="ck-btn ck-btn--secondary ck-btn--sm" onClick={() => choose({ ...project, state: project.state as GitState, last_used_ms: 0 })}>
                  Set up Git here
                </button>
              )}
            </div>
          ) : (
            <div className="ck-menu-note">
              {isolated ? "From the current branch · your uncommitted changes aren't included." : 'Works in this folder.'}
            </div>
          )}
        </>
      )}
      {onOpenStore && (
        <button type="button" role="menuitem" className="ck-menu-item" onClick={onOpenStore}>
          <span>More agents…</span>
        </button>
      )}
      {(onStartMission || onTonight) && <div className="ck-menu-sep" />}
      {onStartMission && (
        <button type="button" role="menuitem" className="ck-menu-item" onClick={onStartMission}>
          <span>Start a mission…</span>
          <span className="ck-menu-hint">Preview</span>
        </button>
      )}
      {onTonight && (
        <button type="button" role="menuitem" className="ck-menu-item" onClick={onTonight}>
          <span>Tonight…</span>
          <span className="ck-menu-hint">Night Shift</span>
        </button>
      )}
    </>
  );
}
