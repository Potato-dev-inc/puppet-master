import { teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// "Queue my agent..." on Maya's push: who does it, and when (Leo's Claude Code is mid-turn).
const s: HarnessScenario = {
  name: 'cockpit-team-queue',
  route: 'cockpit',
  team: teamState(),
  after: async (dom) => {
    await dom.clickText('.ck-team-card button', 'Queue my agent');
    await dom.sleep(200);
  },
};
export default s;
