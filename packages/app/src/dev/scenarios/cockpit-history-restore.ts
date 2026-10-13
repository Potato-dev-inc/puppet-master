import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = {
  name: 'cockpit-history-restore',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.waitFor('.ck-hist-item');
    const rows = document.querySelectorAll<HTMLElement>('.ck-hist-row .ck-btn');
    rows[4]?.click();
    await dom.waitFor('[role="alertdialog"]');
    await dom.sleep(500);
  },
};
export default s;
