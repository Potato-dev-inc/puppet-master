import { NEEDS_START_JOB, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// A queued update stopped on two conflicts and the chosen agent (Codex) runs commands without asking:
// it waits for Start, and then runs in a pane I can watch (security review M6).
const s: HarnessScenario = {
  name: 'cockpit-team-needs-start',
  route: 'cockpit',
  team: teamState({ jobs: [NEEDS_START_JOB], cards: [] }),
};
export default s;
