import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The "Close <pane>?" sheet, then Escape: the sheet must close (it used to ignore the keyboard).
const s: HarnessScenario = {
  name: 'cockpit-sheet-escape',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.click('button[aria-label="Pane actions"]');
    await dom.clickText('[role="menuitem"]', 'Close');
    await dom.waitFor('[role="alertdialog"]');
    dom.key('Escape');
    await dom.sleep(600);
  },
};
export default s;
