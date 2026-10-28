// Assign an issue: a GitHub issue that someone on your list labels (say `puppet`) starts an agent run here.
// Shapes mirror the Rust `issue_intake` types (src-tauri/src/issue_intake/model.rs): config and repo rows are
// snake_case, `RunView` is camelCase (see the spec, section 3.7). The page never gets an issue body, a token or
// pane output: the card carries the issue text, and this file only carries the status around it.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the backend sends when a run or the settings change. The payload is not read: call `intakeView()`. */
export const INTAKE_CHANGED_EVENT = 'issue-intake://changed';

/** `agent` or `agent@account`, split (Rust `AgentPick`). */
export interface AgentPick {
  agent_type: string;
  account: string | null;
}

/** One watched repo (Rust `RepoWatch`). `added_at_ms` is the baseline: older labels are not started by themselves. */
export interface RepoWatch {
  id: string;
  /** `owner/repo`, for example `acme/billing`. */
  owner_repo: string;
  /** The local folder whose `origin` is that repo. */
  folder: string;
  label: string;
  agent: AgentPick;
  turns: number;
  minutes: number;
  runs_per_day: number;
  ask_before_start: boolean;
  ask_before_push: boolean;
  added_at_ms: number;
  paused: boolean;
}

/** Settings for the whole feature (Rust `Config`). Off by default. */
export interface IntakeConfig {
  enabled: boolean;
  /** Extra GitHub logins allowed to start runs, besides you. Lowercase. */
  allowed: string[];
  write_access_counts: boolean;
  parallel: number;
  repos: RepoWatch[];
  /** Issues the person said "Ignore this issue" for: `[owner_repo, number]`. */
  ignored: [string, number][];
}

/** Whether `gh` is ready to talk to GitHub for this person. */
export type GhState = 'ready' | 'missing' | 'signed_out';

export interface GhView {
  state: GhState;
  /** The GitHub login `gh` is signed in as, when ready. */
  login: string | null;
}

/** One row of the repo list in Settings. `agent_label` is the agent's name in plain words ("Codex"). */
export interface RepoStatusView {
  repo: RepoWatch;
  agent_label: string;
  /** A short plain sentence about this repo ("Watching", "Paused", "Skipped: ..."). Set by the backend. */
  status: string;
}

/** What `issue_intake_view` returns. */
export interface IntakeView {
  config: IntakeConfig;
  gh: GhView;
  repos: RepoStatusView[];
  runs: RunView[];
}

/** The states a run can be in (Rust `RunState`, snake_case tags). */
export type RunStateTag =
  | 'skipped'
  | 'asking'
  | 'declined'
  | 'ignored'
  | 'expired'
  | 'queued'
  | 'starting'
  | 'running'
  | 'checking'
  | 'ask_push'
  | 'pushing'
  | 'pr_open'
  | 'no_changes'
  | 'held'
  | 'thrown_away'
  | 'failed';

/** One issue run as the list shows it (Rust `RunView`, camelCase). No issue body. */
export interface RunView {
  id: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  labeledBy: string;
  author: string;
  state: RunStateTag;
  /** The backend's own sentence for the state. Empty means: use `stateText(run)`'s fallback. */
  stateText: string;
  agentLabel: string;
  prUrl: string | null;
  paneId: string | null;
  atMs: number;
  /** Why the start card could not be skipped ("Written by someone who isn't on your list"). */
  forced: string[];
}

/** What `issue_intake_check_folder` says about a folder the person picked. */
export interface FolderCheck {
  owner_repo: string | null;
  default_branch: string | null;
  gh: string;
  why_not: string | null;
}

/** The buttons a run can take from Settings (the same paths as the cards). */
export type IntakeAction = 'start' | 'ask_again' | 'push' | 'throw_away' | 'unignore';

/** Full view: config, GitHub status, repo rows and recent runs. */
export const intakeView = (): Promise<IntakeView> => invoke<IntakeView>('issue_intake_view');

/** Save the settings. Resolves with the new view. The backend checks every value and refuses bad ones. */
export const intakeSave = (config: IntakeConfig): Promise<IntakeView> =>
  invoke<IntakeView>('issue_intake_save', { config });

/** Check a folder before it is added: its `owner/repo`, default branch, and why it cannot be used. */
export const intakeCheckFolder = (folder: string): Promise<FolderCheck> =>
  invoke<FolderCheck>('issue_intake_check_folder', { folder });

/** Do one action on a run: start, ask again, push, throw away, or unignore. Resolves with the run. */
export const intakeAct = (runId: string, action: IntakeAction): Promise<RunView> =>
  invoke<RunView>('issue_intake_act', { runId, action });

/** Check GitHub now instead of waiting for the next minute. */
export const intakePollNow = (): Promise<void> => invoke<void>('issue_intake_poll_now');

const noop: UnlistenFn = () => {};

/** Run `cb` after the backend says a run or the settings changed. Outside the app there is nothing to listen to. */
export function onIntakeChanged(cb: () => void): Promise<UnlistenFn> {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return Promise.resolve(noop);
  return listen(INTAKE_CHANGED_EVENT, () => cb());
}

/** Plain-English text for each state, used when the backend sent no sentence. Never "auto", "merge" or "fix it". */
const FALLBACK: Record<RunStateTag, string> = {
  skipped: 'Skipped',
  asking: 'Waiting for you',
  declined: 'Not now',
  ignored: 'Ignored',
  expired: 'The card timed out',
  queued: 'Waiting for a free slot',
  starting: 'Starting',
  running: 'Working',
  checking: 'Checking the change',
  ask_push: 'Waiting for you to open the draft PR',
  pushing: 'Opening the draft PR',
  pr_open: 'Draft PR open',
  no_changes: 'No changes',
  held: "Puppet Master won't push this. Check the copy yourself.",
  thrown_away: 'Thrown away',
  failed: 'Failed',
};

/** The one-line state of a run: the backend's sentence, else the plain text for its state. */
export function stateText(run: RunView): string {
  const own = run.stateText.trim();
  if (own) return own;
  return FALLBACK[run.state] ?? 'Working';
}

/** "acme/billing · label puppet · Codex · 30 turns · 45 min · 5 a day": one repo row in Settings. */
export function repoLine(repo: RepoWatch, agentLabel: string): string {
  const turns = repo.turns === 1 ? 'turn' : 'turns';
  return [
    repo.owner_repo,
    `label ${repo.label}`,
    agentLabel,
    `${repo.turns} ${turns}`,
    `${repo.minutes} min`,
    `${repo.runs_per_day} a day`,
  ].join(' · ');
}
