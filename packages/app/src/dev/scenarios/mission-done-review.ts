import { PANE_CODEX } from '../fixtures';
import { MISSION_DONE } from '../mission-fixtures';
import type { HarnessScenario } from '../types';

// A finished mission: the Done review (checks, work, test changes, budget) and "Nothing has been
// merged". The person reviews and merges the separate copy.
const s: HarnessScenario = {
  name: 'mission-done-review',
  route: 'cockpit',
  select: PANE_CODEX,
  missions: { enabled: true, list: [MISSION_DONE] },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Mission');
    await dom.waitFor('.ck-mission-done');
    await dom.sleep(500);
  },
};
export default s;
