import { PANE_CODEX } from '../fixtures';
import { RUNNING_LINEAGE } from '../keep-going-fixtures';
import type { HarnessScenario } from '../types';

// The new pane: "Continued from Claude Code · 14 turns · Show brief" above the chat.
const s: HarnessScenario = {
  name: 'keep-going-banner-new',
  route: 'cockpit',
  select: PANE_CODEX,
  keepGoing: { lineage: RUNNING_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-keep-going]');
    await dom.sleep(500);
  },
};
export default s;
