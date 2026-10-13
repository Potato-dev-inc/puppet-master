// Example data for the phone harness (route `phone`): what the PC's bridge would answer.
import type { PaneInfo } from '../lib/tauri-types';
import type { Question } from '../lib/questions';
import { EXAMPLE_DIGEST, PANE_CLAUDE, PANE_CODEX, PANE_OPENCODE, PANE_PWSH, agentsListing } from './fixtures';

const now = Date.now();
const MIN = 60_000;

/** Phone-sized panes (44x30) with their titles. */
export const PHONE_PANES = [
  { id: PANE_CLAUDE, agent_type: 'claude', pid: 4120, status: 'waiting_input', created_at: now - 14 * MIN, last_output_at: now - 2 * MIN, cwd: 'C:\\Users\\dev\\work\\puppet-master', cols: 44, rows: 30, title: 'Fix login redirect' },
  { id: PANE_PWSH, agent_type: 'powershell', pid: 7340, status: 'error', created_at: now - 22 * MIN, last_output_at: now - 3 * MIN, cwd: 'C:\\Users\\dev\\work\\puppet-master', cols: 44, rows: 30, title: null },
  { id: PANE_CODEX, agent_type: 'codex', pid: 5288, status: 'running', created_at: now - 9 * MIN, last_output_at: now - 2_000, cwd: 'C:\\Users\\dev\\work\\billing-service', cols: 44, rows: 30, title: 'Billing tests' },
  { id: PANE_OPENCODE, agent_type: 'opencode', pid: 6012, status: 'idle', created_at: now - 40 * MIN, last_output_at: now - 8 * MIN, cwd: 'C:\\Users\\dev\\work\\docs-site', cols: 44, rows: 30, title: null },
] as unknown as PaneInfo[];

export const PHONE_QUESTION: Question = {
  id: 'q-1',
  agent: 'Claude Code',
  pane_id: PANE_CLAUDE,
  question: 'Keep the old /login route as a redirect, or remove it?',
  choices: [
    { id: 'keep', label: 'Keep redirect' },
    { id: 'remove', label: 'Remove it' },
  ],
  allow_free_text: true,
  context: 'Three pages still link to /login.',
  urgency: 'normal',
  created_at_ms: now - MIN,
  expires_at_ms: now + 60 * MIN,
  state: 'pending',
};

export const PHONE_DIGEST = { ...EXAMPLE_DIGEST, empty: false, previousSeenMs: EXAMPLE_DIGEST.sinceMs };

export const PHONE_PREVIEWS = { [PANE_CODEX]: [{ url: 'http://localhost:5173', port: 5173 }] };

export const PHONE_AGENT_TYPES = (() => {
  const listing = agentsListing();
  const agents = listing.agents.map((a) =>
    a.id === 'cursor_agent' ? { ...a, installed: false } : a.id === 'opencode' ? { ...a, experimental: true } : a,
  );
  return { ...listing, agents };
})();

const ESC = '\x1b';
/** A phone-width screen (44 columns) for the open agent. */
export const PHONE_SCREEN_LINES = [
  `${ESC}[1m* Claude Code${ESC}[0m ${ESC}[2mv2.1${ESC}[0m`,
  `${ESC}[2m  ~/work/puppet-master${ESC}[0m`,
  '',
  `${ESC}[38;5;250m> Fix the login redirect loop${ESC}[0m`,
  '',
  `${ESC}[1m*${ESC}[0m Reading src/pages/Login.tsx`,
  `${ESC}[1m*${ESC}[0m Edit src/pages/Login.tsx ${ESC}[1m+9${ESC}[0m ${ESC}[2m-2${ESC}[0m`,
  `${ESC}[1m*${ESC}[0m Edit src/routes.ts ${ESC}[1m+3${ESC}[0m ${ESC}[2m-1${ESC}[0m`,
  '',
  `${ESC}[1mBash command${ESC}[0m`,
  '  npm test --workspace=@puppet-master/app',
  '',
  'Do you want to proceed?',
  `${ESC}[1m> 1. Yes${ESC}[0m`,
  '  2. Yes, and do not ask again',
  '  3. No, tell Claude what to do differently',
];

const ANSI = new RegExp(`${ESC}\\[[0-9;]*m`, 'g');

export const PHONE_BUFFER: Record<string, string> = {
  [PANE_CLAUDE]: PHONE_SCREEN_LINES.map((l) => l.replace(ANSI, '')).join('\n'),
  [PANE_PWSH]: 'PS C:\\Users\\dev\\work\\puppet-master> npm run build\nnpm ERR! code ELIFECYCLE\nnpm ERR! build failed with exit code 1',
  [PANE_CODEX]: 'Running billing tests\n  PASS invoice.test.ts\n  RUNS refund.test.ts',
  [PANE_OPENCODE]: 'Updated the docs sidebar order.\nAnything else?',
};
