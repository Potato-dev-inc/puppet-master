import { PANE_CODEX } from '../fixtures';
import { MISSION_WORKING } from '../mission-fixtures';
import type { HarnessScenario } from '../types';

// The pane of the agent working on a mission: its Mission tab shows the Progress timeline (turn 7,
// a rollback, a hand-off from Claude Code to Codex), the one progress bar and the budget.
const s: HarnessScenario = {
  name: 'mission-progress',
  route: 'cockpit',
  select: PANE_CODEX,
  missions: { enabled: true, list: [MISSION_WORKING] },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Mission');
    await dom.waitFor('[data-testid="mission-progress"]');
    await dom.sleep(500);
  },
};
export default s;
