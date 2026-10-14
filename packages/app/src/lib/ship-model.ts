// Plain-words helpers for the ship features: no React, no IPC. Tested in ship-model.test.ts.
import type { ChecksReport, GitState, PrPrepared, SetupReport } from './ship-api';

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
export function cannotCreate(p: PrPrepared): { text: string; command?: string } | null {
  if (p.gh === 'missing') {
    return {
      text: "Create PR uses the GitHub command line, and it isn't installed on this PC. Install it from cli.github.com, then try again.",
    };
  }
  if (p.gh === 'signed_out') {
    return { text: 'Sign in to GitHub first. Run this in a terminal, then try again.', command: 'gh auth login' };
  }
  if (!p.has_remote || !p.on_github) {
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
  return setup.status === 'failed' ? `Setting up the copy failed: ${sentence}.` : `${sentence}.`;
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

/** Separate copy is remembered per project (a per-viewer convenience: browser storage is fine). */
export function readCopyPreference(project: string | null): boolean {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Record<string, boolean>;
    return all[project ?? ''] === true;
  } catch {
    return false;
  }
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
