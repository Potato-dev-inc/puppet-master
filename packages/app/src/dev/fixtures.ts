// Realistic example data answered by the mocked Tauri commands.
import type { PaneInfo } from '../lib/tauri-types';
import type { PanePrompt, PaneRunInfo, PaneTranscript } from '../lib/cockpit-api';

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
};

export const EXAMPLE_RUN: PaneRunInfo = {
  handle: 'run_8f3a21c4',
  status: 'completed',
  receipt: {
    task: 'Add retry with backoff to the bridge client',
    outcome: 'completed',
    duration_ms: 412_800,
    verified: ['tsc --noEmit passed', '41 tests passed'],
    not_verified: ['Manual smoke test of the tunnel reconnect'],
    files_changed: ['packages/app/src/lib/bridge.ts', 'packages/app/src/lib/bridge.test.ts'],
    tokens: { input: 48210, output: 6122 },
  },
};

export const EXAMPLE_TRANSCRIPT: PaneTranscript = {
  chunks: [
    { stream: 'user', text: 'Add retry with backoff to the bridge client.' },
    { stream: 'agent', text: 'I will read bridge.ts first, then add a capped exponential backoff.' },
    { stream: 'agent', text: 'Done. Retries use 250ms, 500ms, 1s, capped at 4s, with tests.' },
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

export function standbyStatus() {
  return {
    enabled: true,
    config: { enabled: true },
    counts: { total: 1, ready: 1, starting: 0 },
    standbys: [{ state: 'ready', agent_type: 'claude', headless: false, pane_id: null }],
    blocked: [],
    recent: [],
    stats: { spawned: 3, spawn_failed: 0, claimed: 2, missed: 0, discarded: 0, expired: 0, abandoned: 0 },
    supported: [{ agent_type: 'claude', visible: true, headless: false, experimental: false }],
  };
}

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
