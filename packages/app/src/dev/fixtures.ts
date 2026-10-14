// Realistic example data answered by the mocked Tauri commands.
import type { PaneInfo } from '../lib/tauri-types';
import type { PanePrompt, PaneRunInfo, PaneTranscript } from '../lib/cockpit-api';
import type { ReviewerEntry, ReviewPoll, ReviewStart } from '../lib/second-opinion';

const now = Date.now();

export const PANE_CLAUDE = 'pane-claude-1';
export const PANE_CODEX = 'pane-codex-1';
export const PANE_OPENCODE = 'pane-opencode-1';
export const PANE_PWSH = 'pane-pwsh-1';

export const EXAMPLE_PANES: PaneInfo[] = [
  {
    id: PANE_CLAUDE,
    agent_type: 'claude',
    pid: 4120,
    status: 'waiting_input',
    created_at: now - 14 * 60_000,
    last_output_at: now - 130_000,
    cwd: 'C:\\Users\\dev\\work\\puppet-master',
    cols: 120,
    rows: 32,
  },
  {
    id: PANE_CODEX,
    agent_type: 'codex',
    pid: 5288,
    status: 'running',
    created_at: now - 9 * 60_000,
    last_output_at: now - 2_000,
    cwd: 'C:\\Users\\dev\\work\\billing-service',
    cols: 120,
    rows: 32,
  },
  {
    id: PANE_OPENCODE,
    agent_type: 'opencode',
    pid: 6012,
    status: 'idle',
    created_at: now - 40 * 60_000,
    last_output_at: now - 8 * 60_000,
    cwd: 'C:\\Users\\dev\\work\\docs-site',
    cols: 120,
    rows: 32,
  },
  {
    id: PANE_PWSH,
    agent_type: 'powershell',
    pid: 7340,
    status: 'error',
    created_at: now - 22 * 60_000,
    last_output_at: now - 3 * 60_000,
    cwd: 'C:\\Users\\dev\\work\\puppet-master',
    cols: 120,
    rows: 32,
  },
];

export const EXAMPLE_PROMPT: PanePrompt = {
  promptId: 'prompt-1',
  kind: 'permission',
  text: 'Do you want to run this command?\n  npm test --workspace=@puppet-master/app',
  summary: 'Run npm test in the app workspace',
  choices: [
    { id: 'yes', label: 'Yes' },
    { id: 'yes-always', label: 'Yes, and do not ask again for npm test' },
    { id: 'no', label: 'No, tell Claude what to do differently' },
  ],
  risk: {
    level: 'low',
    headline: 'Run tests',
    reasons: ["It runs your project's tests and does not change anything else."],
  },
};

/** A fixture, not real agent output: a high-risk command prompt for the approval-card scenarios. */
export const HIGH_RISK_PROMPT: PanePrompt = {
  promptId: 'prompt-2',
  kind: 'command_approval',
  text: 'Run this command?\n  rm -rf ../old-site',
  summary: 'Run command: rm -rf ../old-site (in C:\\Users\\dev\\work\\billing-service)',
  choices: [
    { id: 'allow_once', label: 'Allow once' },
    { id: 'allow_all', label: 'Always allow', broad: true },
    { id: 'deny', label: 'Deny' },
  ],
  risk: {
    level: 'high',
    headline: 'Delete a folder outside this project',
    reasons: [
      'It deletes ../old-site and everything in it, and that is outside this project.',
      'It cannot be undone.',
    ],
  },
};

export const EXAMPLE_RUN: PaneRunInfo = {
  handle: 'run_8f3a21c4',
  status: 'completed',
  // The real `RunReceipt` shape (`agent_runs/runtime/receipt.rs`).
  receipt: {
    backend: 'claude',
    model: 'claude-opus-5-5',
    read_only: 'off',
    checks: 'none configured',
    result_capture: 'inferred',
    not_verified: ['no checks configured', 'result read from terminal screen', 'model not confirmed'],
    outcome: 'completed_unchecked',
    build_id: '5fd9f29',
    standby_used: true,
    fallback: { from: 'visible', to: 'headless', reason: 'visible_failed' },
    worktree: {
      path: 'C:\\Users\\dev\\work\\puppet-master\\.puppet-master\\worktrees\\run_8f3a21c4',
      branch: 'pm/run_8f3a21c4',
      base: 'master',
      state: 'unreviewed',
      files_changed: 2,
      additions: 61,
      deletions: 4,
    },
  },
};

// The stream tags are the real ones the backend writes (`agent_runs/transcript.rs` callers).
export const EXAMPLE_TRANSCRIPT: PaneTranscript = {
  chunks: [
    { stream: 'user_task', text: 'Add retry with backoff to the bridge client.' },
    { stream: 'delivery', text: 'Started\nattempt 0: typing; busy=false' },
    { stream: 'assistant_final', text: '● Bash(npm test --workspace=@puppet-master/app)\n  ⎿  41 tests passed' },
    { stream: 'assistant_final', text: 'Done. Retries use 250ms, 500ms, 1s, capped at 4s, with tests.' },
    { stream: 'user_steer', text: 'Also log each retry.' },
    { stream: 'permission', text: 'Allowed once: npm test' },
    { stream: 'execution_failure', text: 'The agent exited before it answered.' },
  ],
};

const ESC = '\x1b';
const SCREEN_LINES = [
  `${ESC}[1m${ESC}[38;5;208m*${ESC}[0m ${ESC}[1mClaude Code${ESC}[0m ${ESC}[2mv2.1${ESC}[0m`,
  `${ESC}[2m  C:\\Users\\dev\\work\\puppet-master${ESC}[0m`,
  '',
  `${ESC}[38;5;245m> Add retry with backoff to the bridge client${ESC}[0m`,
  '',
  `${ESC}[32m*${ESC}[0m Reading packages/app/src/lib/bridge.ts`,
  `${ESC}[32m*${ESC}[0m Edit packages/app/src/lib/bridge.ts ${ESC}[32m+24${ESC}[0m ${ESC}[31m-3${ESC}[0m`,
  '',
  `${ESC}[1mBash command${ESC}[0m`,
  '  npm test --workspace=@puppet-master/app',
  '',
  'Do you want to proceed?',
  `${ESC}[36m> 1. Yes${ESC}[0m`,
  '  2. Yes, and do not ask again for npm test',
  '  3. No, tell Claude what to do differently',
];

export const SCREEN_REPLAY = {
  data: Array.from(new TextEncoder().encode(SCREEN_LINES.join('\r\n'))),
  cols: 120,
  rows: 32,
  offset: 0,
};

export function agentsListing() {
  const row = (id: string, label: string, kind = 'cli') => ({
    id,
    label,
    icon: null,
    kind,
    source: 'code',
    experimental: false,
    dispatchable: kind === 'cli',
    not_dispatchable_reason: kind === 'cli' ? null : 'shell panes cannot run delegated work',
    installed: true,
    install_hint: null,
    modes: { headless: kind === 'cli', visible: true },
    features: { read_only_headless: false, read_only_spawned_pane: false, resumable_headless: false },
    run_options: { trust_workspace: false, allow_shell: false, shell_default_on: false, allowed_commands: false },
    standby: { visible: true, headless: false, experimental: false },
    definition: null,
  });
  const agents = [
    row('claude', 'Claude Code'),
    row('codex', 'Codex'),
    row('opencode', 'OpenCode'),
    row('cursor_agent', 'Cursor Agent'),
    row('powershell', 'PowerShell', 'shell'),
  ];
  return { agents, dispatchable_ids: agents.filter((a) => a.dispatchable).map((a) => a.id) };
}

export function standbyStatus(idleTimeoutSecs = 1200, blocked: unknown[] = []) {
  return {
    enabled: true,
    config: {
      preload: { enabled: true, providers: { claude: 1, codex: 1 } },
      enabled: true,
      backends: {},
      max_total: 3,
      recent_projects: 2,
      recent_hours: 6,
      idle_timeout_secs: idleTimeoutSecs,
    },
    counts: { total: 1, ready: 1, starting: 0 },
    standbys: [
      {
        id: 1,
        state: 'ready',
        agent_type: 'claude',
        project: 'C:\\Users\\dev\\work\\puppet-master',
        headless: false,
        read_only: false,
        model: null,
        health: 'ok',
        age_ms: 42_000,
        ready_for_ms: 30_000,
        idle_expires_in_ms: 1_100_000,
        pane_id: null,
      },
    ],
    blocked,
    recent: [],
    stats: { spawned: 3, spawn_failed: 0, claimed: 2, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
    supported: [{ agent_type: 'claude', visible: true, headless: false, experimental: false }],
  };
}

/** Two Claude accounts and one Codex account, for the accounts scenarios. */
export const EXAMPLE_ACCOUNTS = [
  { id: 'acc_work', agent_id: 'claude', label: 'Work (Max)', home: 'C:\Users\dev\AppData\Roaming\com.puppetmaster.app\accounts\acc_work' },
  { id: 'acc_home', agent_id: 'claude', label: 'Personal', home: 'C:\Users\dev\AppData\Roaming\com.puppetmaster.app\accounts\acc_home' },
  { id: 'acc_codex', agent_id: 'codex', label: 'Team seat', home: 'C:\Users\dev\AppData\Roaming\com.puppetmaster.app\accounts\acc_codex' },
];

export const ACCOUNT_AGENTS = [
  { agent_id: 'claude', home_env: 'CLAUDE_CONFIG_DIR', login_hint: 'Run `claude` and log in, then close it.' },
  { agent_id: 'codex', home_env: 'CODEX_HOME', login_hint: 'Run `codex` and sign in, then close it.' },
];

/** An isolated run's worktree and its changes (Receipt tab "Changes"). */
export const EXAMPLE_WORKTREE = {
  handle: 'refactor-auth',
  branch: 'pm/refactor-auth',
  base: 'main',
  repo: 'C:\Users\dev\work\puppet-master',
  path: 'C:\Users\dev\work\puppet-master\.puppet-master\worktrees\refactor-a-1f2e',
  exists: true,
  state: 'unreviewed',
  summary: { files_changed: 3, additions: 14, deletions: 3 },
};

export const EXAMPLE_WORKTREE_DIFF = {
  branch: 'pm/refactor-auth',
  base: 'main',
  changes: {
    files: [
      { path: 'src/auth/session.ts', status: 'modified', additions: 9, deletions: 3, binary: false },
      { path: 'src/auth/token.ts', status: 'added', additions: 5, deletions: 0, binary: false },
      { path: 'src/auth/legacy.ts', status: 'deleted', additions: 0, deletions: 0, binary: false },
    ],
    files_changed: 3,
    additions: 14,
    deletions: 3,
    truncated: false,
  },
  patch: [
    'diff --git a/src/auth/session.ts b/src/auth/session.ts',
    'index 3b1c..9a7e 100644',
    '--- a/src/auth/session.ts',
    '+++ b/src/auth/session.ts',
    '@@ -12,9 +12,15 @@ export function open(user: User) {',
    '   const now = Date.now();',
    '-  const token = legacyToken(user);',
    '-  store.set(user.id, token);',
    '-  return token;',
    '+  const token = issueToken(user, now);',
    '+  store.set(user.id, token);',
    '+  audit.record("session.open", user.id);',
    '+  return token;',
    ' }',
    'diff --git a/src/auth/token.ts b/src/auth/token.ts',
    'new file mode 100644',
    '--- /dev/null',
    '+++ b/src/auth/token.ts',
    '@@ -0,0 +1,2 @@',
    '+export const issueToken = (u: User, at: number) => sign({ sub: u.id, iat: at });',
    '+',
    '',
  ].join('\n'),
  patch_truncated: false,
};

const ck = (id: string, turn: number, kind: string, label: string, ago: number, files: number, added: number, deleted: number, note = '') => ({
  id,
  commit: `c0ffee${id}`,
  paneId: PANE_CLAUDE,
  turn,
  kind,
  label,
  note,
  atMs: now - ago,
  files,
  added,
  deleted,
});

/** Newest first, as `checkpoints_list` returns it. */
export const EXAMPLE_CHECKPOINTS = {
  state: 'ok',
  message: '',
  items: [
    ck('6', 3, 'answer', 'Before answering a prompt (turn 3)', 90_000, 0, 0, 0),
    ck('5', 3, 'before', 'Before turn 3', 5 * 60_000, 4, 86, 12, 'Add a settings page for notifications'),
    ck('4', 2, 'undo', 'Before undo', 24 * 60_000, 2, 9, 31),
    ck('3', 2, 'after', 'After turn 2', 31 * 60_000, 3, 48, 7),
    ck('2', 2, 'before', 'Before turn 2', 52 * 60_000, 1, 3, 3, 'Fix the failing login test'),
    ck('1', 1, 'before', 'Before turn 1', 2 * 3_600_000, 12, 410, 0, 'Scaffold the app'),
  ],
};

export const EXAMPLE_FILE_CHANGES = [
  { path: 'src/pages/Settings.tsx', status: 'A', added: 64, deleted: 0, binary: false },
  { path: 'src/App.tsx', status: 'M', added: 14, deleted: 2, binary: false },
  { path: 'src/routes.ts', status: 'M', added: 6, deleted: 1, binary: false },
  { path: 'public/bell.png', status: 'A', added: 0, deleted: 0, binary: true },
];

// ---- Toolbelt (Settings > MCP & bridge) ----
import type { ImportCandidate, SyncReport, ToolbeltListing } from '../lib/toolbelt';

export const TOOLBELT_LISTING: ToolbeltListing = {
  servers: [
    { id: 'browser', name: 'Browser', transport: 'stdio', command: 'npx', args: ['-y', '@playwright/mcp'], env: {}, enabled: true, scope: 'global' },
    {
      id: 'postgres',
      name: 'Postgres',
      transport: 'stdio',
      command: 'npx',
      args: ['-y', '@modelcontextprotocol/server-postgres'],
      env: { DATABASE_URL: 'postgres://localhost/app', DB_PASSWORD: '********' },
      enabled: true,
      scope: 'global',
    },
    { id: 'docs', name: 'Library docs', transport: 'http', url: 'https://mcp.context7.com/mcp', headers: {}, enabled: true, scope: 'global' },
    { id: 'linear', name: 'Linear', transport: 'http', url: 'https://mcp.linear.app/mcp', headers: { Authorization: '********' }, enabled: false, scope: 'project' },
  ],
  agents: [
    { id: 'claude', label: 'Claude Code', installed: true, supports_url: true },
    { id: 'codex', label: 'Codex', installed: true, supports_url: true },
    { id: 'opencode', label: 'OpenCode', installed: true, supports_url: true },
    { id: 'cursor_agent', label: 'Cursor Agent', installed: true, supports_url: true },
  ],
  unsupported: [
    { id: 'kilo', label: 'Kilo', reason: "This agent doesn't declare where it keeps MCP servers, so Puppet Master can't write to it." },
  ],
};

export const TOOLBELT_PREVIEW: SyncReport = {
  dry_run: true,
  unsupported: TOOLBELT_LISTING.unsupported,
  hosts: [
    {
      agents: [{ id: 'claude', label: 'Claude Code' }],
      scope: 'global',
      path: 'C:/Users/dev/.claude.json',
      format: 'json',
      status: 'ready',
      reason: null,
      changes: [
        { key: 'pm-toolbelt-browser', name: 'Browser', kind: 'unchanged' },
        { key: 'pm-toolbelt-postgres', name: 'Postgres', kind: 'add' },
        { key: 'pm-toolbelt-docs', name: 'Library docs', kind: 'add' },
      ],
      notes: [],
      diff: '+     "pm-toolbelt-postgres": {\n+       "command": "npx",\n+       "env": { "DB_PASSWORD": "********" }\n+     }',
      written: false,
      backup: null,
    },
    {
      agents: [{ id: 'codex', label: 'Codex' }],
      scope: 'global',
      path: 'C:/Users/dev/.codex/config.toml',
      format: 'toml',
      status: 'ready',
      reason: null,
      changes: [
        { key: 'pm-toolbelt-browser', name: 'Browser', kind: 'update' },
        { key: 'pm-toolbelt-postgres', name: 'Postgres', kind: 'add' },
        { key: 'pm-toolbelt-docs', name: 'Library docs', kind: 'add' },
        { key: 'pm-toolbelt-old-search', name: 'Old search', kind: 'remove' },
      ],
      notes: [],
      diff: '',
      written: false,
      backup: null,
    },
    {
      agents: [
        { id: 'opencode', label: 'OpenCode' },
        { id: 'opencode_native', label: 'OpenCode (API)' },
      ],
      scope: 'global',
      path: 'C:/Users/dev/.config/opencode/opencode.json',
      format: 'json',
      status: 'unchanged',
      reason: null,
      changes: [{ key: 'pm-toolbelt-browser', name: 'Browser', kind: 'unchanged' }],
      notes: [],
      diff: '',
      written: false,
      backup: null,
    },
    {
      agents: [{ id: 'cursor_agent', label: 'Cursor Agent' }],
      scope: 'global',
      path: 'C:/Users/dev/.cursor/mcp.json',
      format: 'json',
      status: 'skipped',
      reason: 'Not installed on this computer, and it has no config file yet.',
      changes: [],
      notes: [],
      diff: '',
      written: false,
      backup: null,
    },
  ],
};

export const TOOLBELT_IMPORT: ImportCandidate[] = [
  {
    reference: 'global|claude|github',
    agents: [{ id: 'claude', label: 'Claude Code' }, { id: 'codex', label: 'Codex' }],
    server: { id: '', name: 'github', transport: 'stdio', command: 'npx', args: ['-y', '@modelcontextprotocol/server-github'], env: { GITHUB_TOKEN: '********' }, enabled: true, scope: 'global' },
    already: false,
  },
  {
    reference: 'global|claude|browser',
    agents: [{ id: 'claude', label: 'Claude Code' }],
    server: { id: '', name: 'browser', transport: 'stdio', command: 'npx', args: ['-y', '@playwright/mcp'], enabled: true, scope: 'global' },
    already: true,
  },
];

// ---- turn summaries and the "While you were away" digest ----

const sum = (
  paneId: string,
  agent: string,
  turn: number,
  endedAgo: number,
  durationMs: number,
  files: { count: number; added: number; deleted: number; paths: string[] } | null,
  sentence: string,
  end: { kind: string; prompt?: string } = { kind: 'idle' },
) => ({
  paneId,
  agent,
  turn,
  startedAtMs: now - endedAgo - durationMs,
  endedAtMs: now - endedAgo,
  durationMs,
  files,
  approvals: [],
  end,
  sentence,
});

export const EXAMPLE_TURN_SUMMARIES = [
  sum(PANE_CLAUDE, 'Claude Code', 2, 31 * 60_000, 21 * 60_000, { count: 3, added: 48, deleted: 7, paths: ['src/login.tsx'] },
    'Claude Code edited 3 files (+48 −7), all checks passed (3/3), now idle.'),
  sum(PANE_CLAUDE, 'Claude Code', 3, 130_000, 4 * 60_000,
    { count: 4, added: 86, deleted: 12, paths: ['src/pages/Settings.tsx', 'src/App.tsx', 'src/routes.ts', 'public/bell.png'] },
    'Claude Code edited 4 files (+86 −12), you approved 1 command, now waiting: allow `npm test`?',
    { kind: 'waiting_input', prompt: 'allow `npm test`?' }),
];

const MIN = 60_000;
export const EXAMPLE_DIGEST = {
  sinceMs: now - 47 * MIN,
  nowMs: now,
  agentsWorked: 3,
  turns: 5,
  lines: [
    {
      paneId: PANE_CLAUDE, agent: 'Claude Code', turns: 2, endedAtMs: now - 2 * MIN, end: 'waiting_input',
      sentence: 'Claude Code edited 4 files (+86 −12), you approved 1 command, now waiting: allow `npm test`?',
    },
    {
      paneId: PANE_CODEX, agent: 'Codex', turns: 2, endedAtMs: now - 9 * MIN, end: 'idle',
      sentence: 'Codex edited 7 files (+212 −34), all checks passed (5/5), now idle.',
    },
    {
      paneId: PANE_OPENCODE, agent: 'OpenCode', turns: 1, endedAtMs: now - 26 * MIN, end: 'idle',
      sentence: 'OpenCode made no file changes, now idle.',
    },
  ],
  needsYou: [
    { paneId: PANE_CLAUDE, agent: 'Claude Code', status: 'waiting_input', prompt: 'Allow `npm test` in puppet-master?' },
    { paneId: PANE_PWSH, agent: 'PowerShell', status: 'error' },
  ],
};

// Second opinion: a reviewer's answer to the Claude pane's worktree changes (dev harness only).
export const EXAMPLE_REVIEWERS: ReviewerEntry[] = [
  { id: 'claude', label: 'Claude Code', ready: false, same: true },
  { id: 'codex', label: 'Codex', ready: true, same: false },
  { id: 'cursor_agent', label: 'Cursor Agent', ready: false, same: false },
];

export const EXAMPLE_REVIEW_START: ReviewStart = {
  paneId: PANE_CLAUDE,
  handle: 'review-1',
  project: 'C:\Users\dev\work\puppet-master',
  reviewer: 'codex',
  reviewerLabel: 'Codex',
  source: 'worktree',
  sourceLabel: 'the worktree changes',
  patch: EXAMPLE_WORKTREE_DIFF.patch,
  patchTruncated: false,
  files: 3,
  additions: 14,
  deletions: 3,
  authorLabel: 'Claude Code',
};

export const EXAMPLE_REVIEW_POLL: ReviewPoll = {
  state: 'done',
  error: null,
  review: {
    verdict: { level: 'concerns', text: 'Two things to fix before merging.' },
    unstructured: null,
    findings: [
      {
        id: 'f1',
        file: 'src/auth/session.ts',
        line: 15,
        severity: 'high',
        text: 'audit.record can throw after the token is stored, leaving a session nobody got. Move it after the return value is safe.',
      },
      {
        id: 'f2',
        file: 'src/auth/session.ts',
        line: 13,
        severity: 'medium',
        text: 'issueToken uses the clock passed in; a skewed clock gives tokens that expire at once.',
      },
      { id: 'f3', file: 'src/auth/token.ts', line: 1, severity: 'low', text: 'Only iat is set. Add an exp claim.' },
      {
        id: 'f4',
        file: 'src/auth/middleware.ts',
        line: 40,
        severity: 'info',
        text: 'Callers of legacyToken in middleware.ts were not updated.',
      },
    ],
  },
};
