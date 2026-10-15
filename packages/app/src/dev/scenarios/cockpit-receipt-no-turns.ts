import type { HarnessScenario } from '../types';
import { PANE_CODEX } from '../fixtures';

// Before the first turn the Receipt has one empty state, and no Second opinion block.
const s: HarnessScenario = {
  name: 'cockpit-receipt-no-turns',
  route: 'cockpit',
  select: PANE_CODEX,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(1200);
  },
};
export default s;
