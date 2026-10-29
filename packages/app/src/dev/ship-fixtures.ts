// Mock answers for the "worktrees that ship" commands (dev harness only). A scenario picks what the
// pretend backend says through `HarnessScenario.ship`.
import type { WorktreeItem } from '../lib/cockpit-api';
import type { WatchView } from '../lib/ci-fix';
import type { ChecksReport, GitInitPlan, GitState, PresetView, PrPrepared, PrRef, RecentProject } from '../lib/ship-api';
import { EXAMPLE_WORKTREE, PANE_CLAUDE } from './fixtures';

export interface ShipScenario {
  recent?: RecentProject[];
  /** What `project_git_state` answers for any folder (default: the folder's entry in `recent`, else a repo). */
  gitState?: GitState;
  plan?: GitInitPlan;
  preset?: PresetView;
  /** Fields merged into the example worktree of the Claude pane. */
  worktree?: Partial<WorktreeItem>;
  prepared?: Partial<PrPrepared>;
  checks?: ChecksReport;
  /** What `ci_fix_offer` answers ("Fix with" / "Watch CI"); default: `WATCH`. */
  watch?: Partial<WatchView>;
}

const now = Date.now();

export const RECENT: RecentProject[] = [
  { path: 'D:\\code\\shop', name: 'shop', state: 'repo', last_used_ms: now - 60_000 },
  { path: 'D:\\code\\puppet-master', name: 'puppet-master', state: 'repo', last_used_ms: now - 3_600_000 },
  { path: 'D:\\code\\landing-page', name: 'landing-page', state: 'folder', last_used_ms: now - 86_400_000 },
  { path: 'D:\\code\\old-experiment', name: 'old-experiment', state: 'missing', last_used_ms: now - 9 * 86_400_000 },
];

export const GIT_PLAN: GitInitPlan = {
  path: 'D:\\code\\landing-page',
  name: 'landing-page',
  state: 'folder',
  commands: ['git init', 'git add -A', 'git commit -m "First snapshot"'],
  files: 48,
  too_big: false,
  gitignore: ['node_modules/', 'target/', 'dist/', '.env', '.env.*'],
};

export const PRESET: PresetView = {
  setup: ['npm ci'],
  copy: ['.env', '.env.local'],
  run: 'npm run dev',
  conductor: null,
  problem: null,
};

export const PREPARED: PrPrepared = {
  gh: 'ready',
  has_remote: true,
  on_github: true,
  default_branch: 'main',
  existing: null,
  title: 'Round cart totals to cents',
  body: [
    '## What changed',
    '',
    'Claude Code changed 3 files and ran 2 commands.',
    '',
    'Task: Round cart totals to cents and add a test.',
    '',
    '3 files changed, +14 \u22123',
    '',
    '- `src/auth/session.ts` (+9 \u22123)',
    '- `src/auth/token.ts` (+4 \u22120)',
    '',
    '## Checks',
    '',
    '14/14 passed (run before this pull request was made)',
    '',
    '## Second opinion',
    '',
    'Codex: no concerns',
  ].join('\n'),
  base: 'main',
  branch: 'pm/refactor-auth',
  agent: 'Claude Code',
  has_uncommitted: true,
};

export const FAILING_CHECKS: ChecksReport = {
  state: 'failing',
  checked_at_ms: now,
  items: [
    { name: 'test (ubuntu)', state: 'fail', workflow: 'ci' },
    { name: 'lint', state: 'fail', workflow: 'ci' },
    { name: 'build', state: 'pass', workflow: 'ci' },
  ],
};

export const PR: PrRef = {
  number: 42,
  url: 'https://github.com/example/shop/pull/42',
  draft: true,
  base: 'main',
  created_at_ms: now - 600_000,
  checks: FAILING_CHECKS,
};

/** The watch Red to Green answers with for this copy's PR. Made-up names and numbers. */
export const WATCH: WatchView = {
  id: 'cw-1a2b3c4d',
  repo: 'example/shop',
  prNumber: 42,
  prUrl: 'https://github.com/example/shop/pull/42',
  branch: 'pm/refactor-auth',
  agentLabel: 'Claude Code',
  state: 'watching',
  stateText: 'Watching CI · 0 of 2 tries used',
  triesUsed: 0,
  triesMax: 2,
  lastFix: null,
  project: 'D:\\code\\shop',
  handle: 'refactor-auth',
};

/** `handled: false` leaves the command to the normal mock. */
export function shipMock(scenario: ShipScenario | undefined, cmd: string, args: Record<string, unknown>): { handled: boolean; value?: unknown } {
  const s = scenario ?? {};
  const yes = (value: unknown) => ({ handled: true, value });
  switch (cmd) {
    case 'project_recent':
      return yes(s.recent ?? RECENT);
    case 'project_git_state':
      return yes(s.gitState ?? (s.recent ?? RECENT).find((p) => p.path === args.path)?.state ?? 'repo');
    case 'project_pick_folder':
      return yes(null);
    case 'project_forget':
      return yes(null);
    case 'project_git_init_plan':
      return yes(s.plan ?? GIT_PLAN);
    case 'project_git_init':
      return yes({ commit: 'a1b2c3d', files: 48, wrote_gitignore: true });
    case 'project_preset_get':
    case 'project_preset_set':
      return yes(s.preset ?? PRESET);
    case 'project_preset_import_conductor':
      return yes({ ...(s.preset ?? PRESET), conductor: null });
    case 'pane_worktree':
      if (!s.worktree) return { handled: false };
      return yes(args.paneId === PANE_CLAUDE ? { ...EXAMPLE_WORKTREE, agent_type: 'claude', ...s.worktree } : null);
    case 'worktree_pr_prepare':
      return yes({ ...PREPARED, ...(s.prepared ?? {}) });
    case 'worktree_pr_create':
      return yes({ ...PR, checks: undefined });
    case 'worktree_pr_checks':
      return yes({ ...PR, checks: s.checks ?? FAILING_CHECKS });
    case 'ci_fix_offer':
      return yes({ ...WATCH, ...(s.watch ?? {}) });
    default:
      return { handled: false };
  }
}
