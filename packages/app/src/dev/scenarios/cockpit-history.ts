import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = {
  name: 'cockpit-history',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.waitFor('.ck-hist-item');
    await dom.clickText('.ck-hist-main', 'Before turn 3');
    await dom.sleep(900);
  },
};
export default s;
