import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// The default: no budget. The header has no chip, exactly as before budgets existed.
const s: HarnessScenario = {
  name: 'cockpit-budget-off',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.waitFor('.ck-pane-head');
    await dom.sleep(300);
  },
};
export default s;
