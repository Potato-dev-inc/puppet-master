import { PANE_CLAUDE } from '../fixtures';
import { FAILED_LINEAGE } from '../keep-going-fixtures';
import type { HarnessScenario } from '../types';

// Codex did not start: "Codex couldn't start. · Try again from the card".
const s: HarnessScenario = {
  name: 'keep-going-banner-failed',
  route: 'cockpit',
  select: PANE_CLAUDE,
  keepGoing: { lineage: FAILED_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-keep-going]');
    await dom.sleep(500);
  },
};
export default s;
