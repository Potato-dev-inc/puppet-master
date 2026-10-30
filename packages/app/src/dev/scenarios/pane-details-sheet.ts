import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The numbers that used to sit in the header (screen size, process id) live under the pane row menu > Details.
const s: HarnessScenario = {
  name: 'pane-details-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    // The pane's menu is the right-click menu of its sidebar row.
    document.querySelector('.ck-row.is-active')?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 60, clientY: 80 }));
    await dom.waitFor('[role="menu"]');
    await dom.clickText('[role="menuitem"]', 'Details');
    await dom.waitFor('.ck-sheet');
    await dom.sleep(400);
  },
};
export default s;
