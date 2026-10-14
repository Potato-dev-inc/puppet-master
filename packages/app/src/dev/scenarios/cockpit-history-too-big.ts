import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = {
  name: 'cockpit-history-too-big',
  route: 'cockpit',
  select: PANE_CLAUDE,
  checkpointsState: 'too_big',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.waitFor('.ck-empty');
    await dom.sleep(500);
  },
};
export default s;
