import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// A folder without Git was picked: "Set up Git here" shows the exact commands before it runs anything.
const s: HarnessScenario = {
  name: 'newagent-git-init-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.waitFor('.ck-chip');
    await dom.click('.ck-chip');
    await dom.waitFor('.ck-menu-item--project');
    await dom.clickText('.ck-menu-item--project', 'landing-page');
    await dom.waitFor('.ck-gitstep-cmds');
    await dom.sleep(500);
  },
};
export default s;
