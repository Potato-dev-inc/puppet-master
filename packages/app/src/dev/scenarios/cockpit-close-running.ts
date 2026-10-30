import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// A working pane: the cockpit Close action asks first.
const s: HarnessScenario = {
  name: 'cockpit-close-running',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    // The pane's menu is the right-click menu of its sidebar row.
    document.querySelector('.ck-row.is-active')?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 60, clientY: 80 }));
    await dom.waitFor('[role="menu"]');
    await dom.clickText('[role="menuitem"]', 'Close');
  },
};
export default s;
