import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The numbers that used to sit in the header (screen size, process id) live under pane actions > Details.
const s: HarnessScenario = {
  name: 'pane-details-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="Pane actions"]');
    await dom.clickText('[role="menuitem"]', 'Details');
    await dom.waitFor('.ck-sheet');
    await dom.sleep(400);
  },
};
export default s;
