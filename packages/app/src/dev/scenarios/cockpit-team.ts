import { teamState, WAITING_JOB } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// Inbox > Team on Leo's side: Maya's Claude Code pushed feat/auth (Queue my agent), Sam opened a
// GitHub PR (Merge when green, coming soon), and one update queued behind Leo's working agent.
const s: HarnessScenario = {
  name: 'cockpit-team',
  route: 'cockpit',
  team: teamState({ jobs: [WAITING_JOB] }),
};
export default s;
