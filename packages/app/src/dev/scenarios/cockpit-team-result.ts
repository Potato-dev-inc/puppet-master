import { READY_JOB, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// The queued update came back: Claude Code merged feat/auth into a separate copy of feat/billing
// and resolved two conflicts. Apply (fast-forward only) / See changes (open) / Discard.
const s: HarnessScenario = {
  name: 'cockpit-team-result',
  route: 'cockpit',
  team: teamState({ jobs: [READY_JOB], cards: [] }),
  after: async (dom) => {
    await dom.clickText('.ck-team-job button', 'See changes');
    await dom.sleep(200);
    await dom.click('.ck-team-job .ck-changes-file');
    await dom.sleep(150);
  },
};
export default s;
