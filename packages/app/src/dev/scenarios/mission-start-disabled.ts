import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// The start sheet with no goal yet: "Start mission" is disabled and the line under it says why.
const s: HarnessScenario = {
  name: 'mission-start-disabled',
  route: 'cockpit',
  select: PANE_CLAUDE,
  window: { width: 1440, height: 1000 },
  missions: { enabled: true, list: [] },
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(300);
    await dom.clickText('.ck-menu-item', 'Start a mission');
    await dom.waitFor('.ck-mission-blocked');
    await dom.sleep(400);
  },
};
export default s;
