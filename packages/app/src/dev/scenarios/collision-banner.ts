import { PANE_CLAUDE } from '../fixtures';
import { ALERTS } from '../collision-fixtures';
import type { HarnessScenario } from '../types';

// Collision Alert, the row above Claude Code's chat: "Codex also changed src/auth.ts · Lines 40-80 · Open Codex · Ignore".
const s: HarnessScenario = {
  name: 'collision-banner',
  route: 'cockpit',
  select: PANE_CLAUDE,
  collision: { alerts: ALERTS },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-collision-banner]');
    await dom.sleep(500);
  },
};
export default s;
