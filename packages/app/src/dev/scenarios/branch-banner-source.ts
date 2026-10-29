import { PANE_CLAUDE } from '../fixtures';
import { SOURCE_LINEAGE } from '../branch-fixtures';
import type { HarnessScenario } from '../types';

// The source pane: "Branches: Codex · Cursor Agent (couldn't start)" and the failure line above the chat.
const s: HarnessScenario = {
  name: 'branch-banner-source',
  route: 'cockpit',
  select: PANE_CLAUDE,
  branch: { lineage: SOURCE_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.waitFor('[data-branch-banner="source"]');
    await dom.sleep(500);
  },
};
export default s;
