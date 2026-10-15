import type { HarnessScenario } from '../types';
import { PANE_CODEX } from '../fixtures';

const s: HarnessScenario = {
  name: 'cockpit-receipt-empty',
  route: 'cockpit',
  select: PANE_CODEX,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(1200);
  },
};
export default s;
