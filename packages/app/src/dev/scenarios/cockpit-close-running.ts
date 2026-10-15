import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// A working pane: the cockpit Close action asks first.
const s: HarnessScenario = {
  name: 'cockpit-close-running',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="Pane actions"]');
    await dom.clickText('[role="menuitem"]', 'Close');
  },
};
export default s;
