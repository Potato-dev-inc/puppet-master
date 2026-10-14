// Example Team data for the phone harness: what the PC's `GET /team/cards` would answer.
// Built from the desktop fixtures (dev/team-fixtures.ts) so both screens show the same people.
import type { TeamCardsReply } from '../lib/bridge-types';
import type { TeamCard, TeamJob } from '../lib/team';
import { MAYA_PUSH, READY_JOB, SAM_PR, TEAM_CARDS } from './team-fixtures';

const MIN = 60_000;
const now = Date.now();

/** Maya's push (Queue my agent: the project has an open agent) and Sam's PR (a plain pull works). */
export const PHONE_TEAM_CARDS: TeamCard[] = [
  { ...TEAM_CARDS[0], id: MAYA_PUSH.id, receivedAtMs: now - 4 * MIN },
  {
    id: SAM_PR.id,
    event: SAM_PR,
    receivedAtMs: now - 26 * MIN,
    state: 'new',
    local: { ...TEAM_CARDS[0].local!, canPull: true, canQueue: false },
    panes: [],
  },
];

/** An earlier update the agent already finished in a separate copy. */
export const PHONE_TEAM_JOB: TeamJob = {
  ...READY_JOB,
  id: 'job-ready',
  cardId: 'older-card',
  who: 'Priya',
  theirBranch: 'fix/tax-rounding',
  conflicts: [],
  message: 'Claude Code merged fix/tax-rounding with no conflicts.',
  stats: { files: 2, additions: 31, deletions: 6 },
  createdAtMs: now - 12 * MIN,
  updatedAtMs: now - 2 * MIN,
};

export function phoneTeamReply(opts: { can?: { dismiss: boolean; act: boolean }; missing?: number } = {}): TeamCardsReply {
  return {
    enabled: true,
    active: true,
    missing: opts.missing ?? 0,
    error: null,
    cards: PHONE_TEAM_CARDS,
    jobs: [PHONE_TEAM_JOB],
    can: opts.can ?? { dismiss: true, act: true },
  };
}

/** What a phone gets while Team is off on the PC. */
export const PHONE_TEAM_OFF: TeamCardsReply = {
  enabled: false,
  active: false,
  missing: 0,
  error: null,
  cards: [],
  jobs: [],
  can: { dismiss: false, act: false },
};
