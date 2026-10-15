import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The bottom of the Receipt tab: the run ID sits under a "Details" disclosure (opened here).
const s: HarnessScenario = {
  name: 'cockpit-receipt-details',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(1500);
    const details = document.querySelector<HTMLDetailsElement>('details.ck-details');
    if (details) details.open = true;
    const scroller = document.querySelector<HTMLElement>('.ck-scroll');
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
    await dom.sleep(300);
  },
};
export default s;
