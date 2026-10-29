import { PANE_CODEX } from '../fixtures';
import { NEW_LINEAGE } from '../branch-fixtures';
import type { HarnessScenario } from '../types';

// The new pane: "Branched from Claude Code · after turn 5 · 14:02 · Show brief · Open original" above the chat.
const s: HarnessScenario = {
  name: 'branch-banner',
  route: 'cockpit',
  select: PANE_CODEX,
  branch: { lineage: NEW_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-branch-banner]');
    await dom.sleep(500);
  },
};
export default s;
