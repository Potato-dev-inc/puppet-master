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
