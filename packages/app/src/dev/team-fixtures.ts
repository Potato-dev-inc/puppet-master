// Example Team data for the harness (dev only): a teammate's push, my unsent push, queued updates.
import type { TeamCard, TeamDraft, TeamEvent, TeamJob, TeamState } from '../lib/team';
import { PANE_CLAUDE } from './fixtures';

const now = Date.now();
const KEY = 'github.com/acme/billing-service';

export const MAYA_PUSH: TeamEvent = {
  v: 1,
  id: '01JA0MAYAPUSH0000000000001',
  team: 'acme',
  project: { key: KEY, name: 'billing-service' },
  actor: { member: 'maya', device: 'maya-desktop', display: 'Maya', agent: { label: 'Claude Code' } },
  kind: 'pushed',
  atMs: now - 4 * 60_000,
  subject: {
    branch: 'feat/auth',
    commit: '3f9c2a1d8e7b6c5a4f3e2d1c0b9a8f7e6d5c4b3a',
    paths: ['src/auth.ts', 'src/session.ts', 'src/token.ts'],
    stats: { files: 3, additions: 84, deletions: 12 },
  },
  note: 'Touches the session model, careful with your billing branch',
  tierHint: 'ambient',
  dedupeKey: KEY + '@feat/auth:pushed',
  expiresAtMs: now + 7 * 86_400_000,
  source: 'git',
};

export const SAM_PR: TeamEvent = {
  ...MAYA_PUSH,
  id: '01JA0SAMPR0000000000000002',
  actor: { member: 'sam', device: 'sam-laptop', display: 'Sam', agent: { label: 'Codex' } },
  kind: 'pr_opened',
  atMs: now - 26 * 60_000,
  subject: {
    branch: 'feat/invoices',
    commit: 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678',
    pr: { number: 142, title: 'Invoice PDF export', url: 'https://github.com/acme/billing-service/pull/142', state: 'open', draft: false },
    stats: { files: 6, additions: 212, deletions: 34 },
  },
  note: undefined,
  dedupeKey: KEY + '#142:pr_opened',
};

const local = {
  projectPath: 'C:\\Users\\dev\\work\\billing-service',
  currentBranch: 'feat/billing',
  dirty: false,
  upToDate: false,
  canPull: false,
  canQueue: true,
  note: null,
};

export const TEAM_CARDS: TeamCard[] = [
  {
    id: MAYA_PUSH.id,
    event: MAYA_PUSH,
    receivedAtMs: now - 4 * 60_000,
    state: 'new',
    local,
    panes: [{ paneId: PANE_CLAUDE, label: 'Claude Code', status: 'running', title: 'Billing page' }],
  },
  { id: SAM_PR.id, event: SAM_PR, receivedAtMs: now - 26 * 60_000, state: 'new', local, panes: [] },
];

export const MY_DRAFT: TeamDraft = {
  id: '01JA0LEOPUSH00000000000003',
  repo: local.projectPath,
  createdAtMs: now - 3_000,
  event: {
    ...MAYA_PUSH,
    id: '01JA0LEOPUSH00000000000003',
    actor: { member: 'leo', device: 'leo-desktop', display: 'Leo', agent: { label: 'Claude Code' } },
    atMs: now - 3_000,
    subject: { branch: 'feat/billing', commit: '9e8d7c6b5a4f3e2d1c0b9a8f7e6d5c4b3a291807', stats: { files: 4, additions: 120, deletions: 9 } },
    note: undefined,
    dedupeKey: KEY + '@feat/billing:pushed',
  },
};

export const READY_JOB: TeamJob = {
  id: 'job-ready',
  cardId: MAYA_PUSH.id,
  project: local.projectPath,
  paneId: PANE_CLAUDE,
  agentType: 'claude',
  agentLabel: 'Claude Code',
  who: 'Maya',
  theirBranch: 'feat/auth',
  commit: '3f9c2a1d8e7b6c5a4f3e2d1c0b9a8f7e6d5c4b3a',
  note: MAYA_PUSH.note,
  into: 'feat/billing',
  state: 'ready',
  handle: '5c1e0f3a-team-update',
  copyBranch: 'pm/5c1e0f3a-team-update',
  conflicts: ['src/auth.ts', 'src/session.ts'],
  agentRan: true,
  stats: { files: 3, additions: 91, deletions: 14 },
  message: 'Claude Code merged feat/auth and resolved 2 conflicts.',
  when: 'idle',
  createdAtMs: now - 6 * 60_000,
  updatedAtMs: now - 60_000,
};

export const WAITING_JOB: TeamJob = {
  ...READY_JOB,
  id: 'job-waiting',
  state: 'waiting',
  conflicts: [],
  agentRan: false,
  stats: null,
  message: null,
  copyBranch: null,
  theirBranch: 'feat/invoices',
};

export function teamState(partial: Partial<TeamState> = {}): TeamState {
  return {
    enabled: true,
    active: true,
    me: 'Leo',
    members: ['Maya', 'Sam'],
    transport: 'folder',
    error: null,
    drafts: [],
    cards: TEAM_CARDS,
    jobs: [],
    ...partial,
  };
}

export const TEAM_DIFF = {
  branch: 'pm/5c1e0f3a-team-update',
  base: 'feat/billing',
  changes: {
    files: [
      { path: 'src/auth.ts', status: 'modified' as const, additions: 52, deletions: 9, binary: false },
      { path: 'src/session.ts', status: 'modified' as const, additions: 27, deletions: 5, binary: false },
      { path: 'src/token.ts', status: 'added' as const, additions: 12, deletions: 0, binary: false },
    ],
    files_changed: 3,
    additions: 91,
    deletions: 14,
    truncated: false,
  },
  patch: [
    'diff --git a/src/auth.ts b/src/auth.ts',
    '--- a/src/auth.ts',
    '+++ b/src/auth.ts',
    '@@ -10,7 +10,12 @@ export async function signIn(user: User) {',
    '   const session = await openSession(user);',
    '-  return session.token;',
    '+  const token = await refreshIfStale(session.token);',
    '+  session.token = token;',
    '+  return token;',
    ' }',
  ].join('\n'),
  patch_truncated: false,
};
