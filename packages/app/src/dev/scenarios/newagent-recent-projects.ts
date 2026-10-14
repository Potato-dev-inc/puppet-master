import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The project chip opened: Recent projects with each folder's Git state, a deleted folder greyed, Choose folder.
const s: HarnessScenario = {
  name: 'newagent-recent-projects',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.waitFor('.ck-chip');
    await dom.click('.ck-chip');
    await dom.waitFor('.ck-menu-item--project');
    await dom.sleep(500);
  },
};
export default s;
