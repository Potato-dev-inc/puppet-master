import { PANE_CLAUDE } from '../fixtures';
import { RUNNING_LINEAGE } from '../keep-going-fixtures';
import type { HarnessScenario } from '../types';

// The old pane after a hand-over: "Continued on Codex · 17:02 · Open" above the chat.
const s: HarnessScenario = {
  name: 'keep-going-banner',
  route: 'cockpit',
  select: PANE_CLAUDE,
  keepGoing: { lineage: RUNNING_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-keep-going]');
    await dom.sleep(500);
  },
};
export default s;
