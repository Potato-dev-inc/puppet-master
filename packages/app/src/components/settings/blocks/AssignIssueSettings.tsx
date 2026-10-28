// Settings > Labs > Assign an issue. Off by default. Shows whether GitHub (gh) is ready, the repositories Puppet
// Master watches, who may start a run, how many run at once, and the recent runs. Every change is one
// `intakeSave` (the backend checks it again) or one `intakeAct`. The copy says what happens and what does not.
import { useCallback, useEffect, useState, type MouseEvent } from 'react';
import { tauri } from '../../../lib/tauri';
import {
  intakeAct,
  intakePollNow,
  intakeSave,
  intakeView,
  onIntakeChanged,
  repoLine,
  stateText,
  type GhView,
  type IntakeAction,
  type IntakeConfig,
  type IntakeView,
  type RepoWatch,
  type RunView,
} from '../../../lib/issue-intake';
import { ConfirmSheet, NumberControl } from '../controls';
import { Btn, FieldInput, Note, SettingBlock, SettingRow, SettingToggle } from '../settings-ui';
import { AddRepoSheet, REPOS_CAP } from './AddRepoSheet';

/** Most extra GitHub names on the list (the backend checks it too). */
export const NAMES_CAP = 50;
/** How many recent runs the list shows. */
export const RECENT_CAP = 20;
const NAME_RE = /^[a-z0-9-]{1,39}(\[bot\])?$/;

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** One GitHub name, lowercased, with a leading "@" dropped. A bot name is `name[bot]`. */
export function gitHubName(raw: string): { name: string } | { error: string } {
  const name = raw.trim().replace(/^@/, '').toLowerCase();
  if (!NAME_RE.test(name)) return { error: 'Type a GitHub name, like someone. Letters, numbers and dashes only.' };
  return { name };
}

/** The GitHub line under the switch: signed in as whom, or what to install. */
export function ghLine(gh: GhView): string {
  if (gh.state === 'ready') return gh.login ? `Signed in to GitHub as @${gh.login} (gh)` : 'Signed in to GitHub (gh)';
  return 'Install the GitHub CLI and run gh auth login. Nothing is checked until then.';
}

/** The number in a pull request link ("/pull/57" -> "57"), only for a github.com link. */
export function prNumber(url: string | null): string | null {
  if (!url || !url.startsWith('https://github.com/')) return null;
  const m = /\/pull\/(\d+)/.exec(url);
  return m ? m[1] : null;
}

function RunRow({ run, busy, onAct }: { run: RunView; busy: boolean; onAct: (action: IntakeAction) => void }) {
  const pr = prNumber(run.prUrl);
  const openPr = (e: MouseEvent<HTMLAnchorElement>) => {
    // In the desktop app the system browser opens it; elsewhere the link works as usual.
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      e.preventDefault();
      void tauri.openExternalUrl(run.prUrl ?? '');
    }
  };
  return (
    <li className="st-audit-row" aria-label={`#${run.number} ${run.title}`}>
      <span className="st-audit-when">#{run.number}</span>
      <span className="st-audit-what">
        {run.title} · {pr && run.prUrl ? (
          <a className="st-link" href={run.prUrl} target="_blank" rel="noreferrer" onClick={openPr}>
            Draft PR #{pr}
          </a>
        ) : (
          stateText(run)
        )}{' '}
        {run.state === 'declined' && (
          <Btn small disabled={busy} onClick={() => onAct('ask_again')}>
            Ask again
          </Btn>
        )}
        {run.state === 'ignored' && (
          <Btn small disabled={busy} onClick={() => onAct('unignore')}>
            Stop ignoring
          </Btn>
        )}
      </span>
      <span className="st-audit-who">{run.agentLabel}</span>
    </li>
  );
}

export function AssignIssueSettings({ onSaved, projectPath = null }: { onSaved?: () => void; projectPath?: string | null }) {
  const [view, setView] = useState<IntakeView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The repo sheet: `'new'` to add one, a repo to edit it, null when closed. */
  const [sheet, setSheet] = useState<RepoWatch | 'new' | null>(null);
  const [removing, setRemoving] = useState<RepoWatch | null>(null);
  /** The GitHub name being typed; null while the add box is closed. */
  const [naming, setNaming] = useState<string | null>(null);
  const [nameError, setNameError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setView(await intakeView());
      setError(null);
    } catch (e) {
      // No desktop shell (a browser preview) or the backend is not ready: say so, do not crash.
      setError(`Could not read this setting: ${errText(e)}`);
    }
  }, []);

  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    void load();
    void onIntakeChanged(() => void load())
      .then((off) => {
        if (alive) stop = off;
        else off();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      stop?.();
    };
  }, [load]);

  /** Write the whole config. Throws the backend's message; callers decide where it shows. */
  const persist = async (next: IntakeConfig) => {
    setView(await intakeSave(next));
    onSaved?.();
  };

  const change = async (next: IntakeConfig) => {
    setBusy(true);
    setError(null);
    try {
      await persist(next);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  const pollNow = async () => {
    setError(null);
    try {
      await intakePollNow();
      await load();
    } catch (e) {
      setError(errText(e));
    }
  };

  const doRun = async (run: RunView, action: IntakeAction) => {
    setBusy(true);
    setError(null);
    try {
      await intakeAct(run.id, action);
      await load();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  if (!view) return error ? <Note role="alert">{error}</Note> : <Note role="status">Loading…</Note>;

  const { config, gh } = view;
  const names = config.allowed;
  const addName = () => {
    const parsed = gitHubName(naming ?? '');
    if ('error' in parsed) return setNameError(parsed.error);
    if (gh.login && parsed.name === gh.login.toLowerCase()) return setNameError('That is you already.');
    if (names.includes(parsed.name)) return setNameError('That name is already on the list.');
    if (names.length >= NAMES_CAP) return setNameError(`You can add up to ${NAMES_CAP} names.`);
    setNameError(null);
    setNaming(null);
    void change({ ...config, allowed: [...names, parsed.name] });
  };

  return (
    <>
      {error && <Note role="alert">{error}</Note>}
      <SettingToggle
        label="Watch GitHub issues"
        description="Puppet Master checks GitHub about once a minute with your own gh sign-in. Nothing is watched until you add a repository below."
        checked={config.enabled}
        disabled={busy}
        onChange={(on) => void change({ ...config, enabled: on })}
      />
      <SettingRow label="GitHub" description={ghLine(gh)}>
        <Btn small disabled={busy || gh.state !== 'ready'} onClick={() => void pollNow()}>
          Check now
        </Btn>
      </SettingRow>

      <SettingBlock label="Repositories" description="Only these are watched. Each one is a folder on this PC whose GitHub remote is that repository.">
        {view.repos.length === 0 && <Note>No repositories yet.</Note>}
        <ul className="st-devices" aria-label="Watched repositories">
          {view.repos.map((row) => (
            <li key={row.repo.id} className="st-device" aria-label={row.repo.owner_repo}>
              <div className="st-device-head">
                <div className="st-label">
                  <span>{row.repo.owner_repo}</span>
                  <small>{repoLine(row.repo, row.agent_label)}</small>
                </div>
                <div className="st-control">
                  <Btn small disabled={busy} onClick={() => setSheet(row.repo)}>
                    Edit
                  </Btn>
                  <Btn
                    small
                    disabled={busy}
                    onClick={() =>
                      void change({ ...config, repos: config.repos.map((r) => (r.id === row.repo.id ? { ...r, paused: !r.paused } : r)) })
                    }
                  >
                    {row.repo.paused ? 'Resume' : 'Pause'}
                  </Btn>
                  <Btn small disabled={busy} onClick={() => setRemoving(row.repo)}>
                    Remove
                  </Btn>
                </div>
              </div>
              <p className="st-note">{row.status}</p>
            </li>
          ))}
        </ul>
        <Btn disabled={busy || config.repos.length >= REPOS_CAP} onClick={() => setSheet('new')}>
          + Add a repository
        </Btn>
        {config.repos.length >= REPOS_CAP && <Note>You can watch up to {REPOS_CAP} repositories. Remove one to add another.</Note>}
      </SettingBlock>

      <SettingBlock
        label="Who can start a run"
        description="You can always start a run. Add GitHub names for people you trust. A label from anyone else is shown under Recent and starts nothing."
      >
        <ul className="st-abilities" aria-label="Who can start a run">
          <li className="st-ability">
            <div className="st-label">
              <span>You</span>
              <small>{gh.login ? `@${gh.login}` : 'Your GitHub sign-in'}</small>
            </div>
          </li>
          {names.map((name) => (
            <li key={name} className="st-ability">
              <div className="st-label">
                <span>@{name}</span>
              </div>
              <Btn small disabled={busy} onClick={() => void change({ ...config, allowed: names.filter((n) => n !== name) })}>
                Remove
              </Btn>
            </li>
          ))}
        </ul>
        {naming === null ? (
          <Btn
            disabled={busy || names.length >= NAMES_CAP}
            onClick={() => {
              setNaming('');
              setNameError(null);
            }}
          >
            + Add a GitHub name
          </Btn>
        ) : (
          <div className="st-path">
            <FieldInput
              aria-label="GitHub name"
              className="font-mono"
              value={naming}
              placeholder="someone"
              onChange={(e) => setNaming(e.target.value)}
            />
            <Btn primary disabled={busy} onClick={addName}>
              Add
            </Btn>
            <Btn
              onClick={() => {
                setNaming(null);
                setNameError(null);
              }}
            >
              Cancel
            </Btn>
          </div>
        )}
        {names.length >= NAMES_CAP && <Note>You can add up to {NAMES_CAP} names.</Note>}
        {nameError && <Note role="alert">{nameError}</Note>}
        <SettingToggle
          label="Anyone with write access to the repo"
          description="Off is safer. When on, anyone who can push to the repository can start a run by adding the label."
          checked={config.write_access_counts}
          disabled={busy}
          onChange={(on) => void change({ ...config, write_access_counts: on })}
        />
      </SettingBlock>

      <SettingRow label="Runs at once" description="How many issue runs can work at the same time. Extra approved issues wait in line.">
        <NumberControl
          aria-label="Runs at once"
          min={1}
          max={3}
          value={config.parallel}
          disabled={busy}
          onCommit={(next) => change({ ...config, parallel: next })}
        />
      </SettingRow>

      <SettingBlock label="Recent" description={`The last ${RECENT_CAP} runs and skipped labels.`}>
        {view.runs.length === 0 ? (
          <Note>No runs yet.</Note>
        ) : (
          <ul className="st-audit" aria-label="Recent runs">
            {view.runs.slice(0, RECENT_CAP).map((run) => (
              <RunRow key={run.id} run={run} busy={busy} onAct={(action) => void doRun(run, action)} />
            ))}
          </ul>
        )}
      </SettingBlock>

      <Note>
        Puppet Master works on an issue only after someone on your list adds the label. The agent can't push; Puppet Master opens a
        draft pull request and never merges.
      </Note>

      {sheet !== null && (
        <AddRepoSheet
          editing={sheet === 'new' ? null : sheet}
          repos={config.repos}
          projectPath={projectPath}
          onCancel={() => setSheet(null)}
          onSave={async (repo) => {
            const next = sheet === 'new' ? [...config.repos, repo] : config.repos.map((r) => (r.id === repo.id ? repo : r));
            await persist({ ...config, repos: next });
            setSheet(null);
          }}
        />
      )}
      {removing && (
        <ConfirmSheet
          title={`Stop watching ${removing.owner_repo}?`}
          body="Puppet Master stops watching this repository. Your folder and its files are not touched."
          confirmLabel="Stop watching"
          onCancel={() => setRemoving(null)}
          onConfirm={() => {
            const gone = removing;
            setRemoving(null);
            void change({ ...config, repos: config.repos.filter((r) => r.id !== gone.id) });
          }}
        />
      )}
    </>
  );
}
