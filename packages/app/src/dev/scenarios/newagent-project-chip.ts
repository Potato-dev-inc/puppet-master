import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The "+" menu opens with the project chip at the top (the folder step). It names the selected pane's project.
const s: HarnessScenario = {
  name: 'newagent-project-chip',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.waitFor('.ck-chip');
    await dom.sleep(500);
  },
};
export default s;
