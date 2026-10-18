// Plain-words helpers for the ship features: no React, no IPC. Tested in ship-model.test.ts.
import type { ChecksReport, GitState, PrPrepared, RecentProject, SetupReport } from './ship-api';

/** `2 GitHub checks failed: test (ubuntu), lint` and friends. */
export function checksSentence(report: ChecksReport | undefined | null): string {
  if (!report) return 'Checking GitHub…';
  const failed = report.items.filter((i) => i.state === 'fail');
  const pending = report.items.filter((i) => i.state === 'pending');
  switch (report.state) {
    case 'failing':
      return `${failed.length} GitHub ${failed.length === 1 ? 'check' : 'checks'} failed: ${failed.map((i) => i.name).join(', ')}`;
    case 'pending':
      return `GitHub checks are running (${pending.length} left)`;
    case 'passing':
      return `All ${report.items.length} GitHub checks passed`;
    default:
      return 'GitHub has no checks for this pull request yet';
  }
}

/** Why Create PR cannot run yet, in words a non-expert understands; `null` when it can. */
export function cannotCreate(p: PrPrepared): { text: string; command?: string; heading?: string; byHand?: boolean } | null {
  if (p.gh === 'missing') {
    return {
      text: "Create PR uses the GitHub command line, and it isn't installed on this PC. Install it from cli.github.com, then try again.",
      byHand: true,
    };
  }
  if (p.gh === 'signed_out') {
    return { text: 'Sign in to GitHub first. Run this in a terminal, then try again.', command: 'gh auth login' };
  }
  if (!p.has_remote) {
    // Short on purpose: say why, then give the exact commands for whoever adds a remote later.
    return {
      text: "This project has no GitHub remote yet, so Create PR can't push. Merge into the main branch instead.",
      heading: 'Once you add a remote (git remote add origin <address>), run:',
      byHand: true,
    };
  }
  if (!p.on_github) {
    return { text: "This project isn't on GitHub. Merge into the main branch instead, or add a GitHub remote." };
  }
  return null;
}

/**
 * The commands to run by hand when `gh` is missing: push the branch, then open the pull request
 * from the page GitHub prints (or with `gh` once it is installed).
 */
export function copyCommands(branch: string, base: string, title: string): string {
  const safe = title.replace(/"/g, "'");
  return [`git push --set-upstream origin ${branch}`, `gh pr create --draft --base ${base} --head ${branch} --title "${safe}"`].join('\n');
}

/** One line about how the copy was set up, for the receipt and the Changes tab. */
export function setupLine(setup: SetupReport | undefined | null): string | null {
  if (!setup) return null;
  const parts: string[] = [];
  for (const s of setup.steps) {
    parts.push(s.ok ? `ran ${s.command}` : s.timed_out ? `${s.command} took too long and was stopped` : `${s.command} stopped with an error`);
  }
  if (setup.copied.length > 0) parts.push(`copied ${setup.copied.join(', ')} (never committed)`);
  if (parts.length === 0) return null;
  const text = parts.join('; ');
  const sentence = text.charAt(0).toUpperCase() + text.slice(1);
  return setup.status === 'failed' ? `Setting up the copy failed: ${text}.` : `${sentence}.`;
}

/** The sentence for a folder in the recents list. */
export function stateHint(state: GitState): string {
  switch (state) {
    case 'missing':
      return 'Folder not found';
    case 'folder':
      return 'No Git yet';
    case 'no_commits':
      return 'No snapshot yet';
    default:
      return '';
  }
}

/** Separate copies need a Git repository with a first commit. */
export const supportsCopies = (state: GitState | undefined): boolean => state === undefined || state === 'repo';

/** The project folder a pane works in: a separate copy's folder maps back to its origin project. */
export function projectOfCwd(cwd: string | undefined | null): { path: string; name: string } | null {
  if (!cwd) return null;
  const slashed = cwd.replace(/\\/g, '/');
  const at = slashed.indexOf('/.puppet-master/worktrees/');
  const path = at > 0 ? cwd.slice(0, at) : cwd;
  const name = path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
  return { path, name };
}

const KEY = 'pm.separate-copy';

/**
 * What this person chose for a project last time: `true`, `false`, or `undefined` when they never chose.
 * (A per-viewer convenience: browser storage is fine.)
 */
export function readStoredCopyPreference(project: string | null): boolean | undefined {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Record<string, boolean>;
    const value = all[project ?? ''];
    return typeof value === 'boolean' ? value : undefined;
  } catch {
    return undefined;
  }
}

/** Separate copy is remembered per project; no remembered choice reads as off. */
export function readCopyPreference(project: string | null): boolean {
  return readStoredCopyPreference(project) === true;
}

/**
 * Whether the "Separate copy" switch starts on. It starts OFF: the folder the person selected is the
 * folder the agent works in. (Until 2026-10-18 it started on for a Git project; that was reversed.)
 * A choice the person made for this project, in either direction, always wins; only a clicked switch is
 * ever stored, so "never chose" stays `undefined` and reads as off.
 */
export function copyDefault(remembered: boolean | undefined): boolean {
  return remembered ?? false;
}

/** True when `path` is the folder of a separate copy (`<project>/.puppet-master/worktrees/<id>`). */
const isCopyPath = (path: string): boolean => path.replace(/\\/g, '/').indexOf('/.puppet-master/worktrees/') > 0;

/**
 * The recent-projects list as the person should see it: a separate copy's folder is mapped back to the
 * project it came from, and each real folder appears once (first, i.e. most recent, wins). The backend
 * already does this; this keeps a copy off the screen even if one ever slips through.
 */
export function realRecents(list: RecentProject[]): RecentProject[] {
  const seen = new Set<string>();
  const out: RecentProject[] = [];
  for (const item of list) {
    const real = isCopyPath(item.path) ? projectOfCwd(item.path) : null;
    const path = real ? real.path : item.path;
    const key = path.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(real ? { ...item, path, name: real.name } : item);
  }
  return out;
}

/**
 * The folder "+" starts in when nothing else names one: the newest real folder the person used. A folder
 * that only ever had runs (`last_used_ms` 0) or no longer exists does not count.
 */
export function lastRealFolder(list: RecentProject[]): { path: string; name: string; state: GitState } | null {
  const found = realRecents(list).find((item) => item.last_used_ms > 0 && item.state !== 'missing');
  return found ? { path: found.path, name: found.name, state: found.state } : null;
}

export function writeCopyPreference(project: string | null, on: boolean): void {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Record<string, boolean>;
    all[project ?? ''] = on;
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* storage can be blocked: the switch just is not remembered */
  }
}
