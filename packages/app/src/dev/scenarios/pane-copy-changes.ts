import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';

// A pane working in a separate copy: the header names the project the copy was made from (never the
// generated folder), and Changes says "Separate copy of main" with the branch under Details.
const s: HarnessScenario = {
  name: 'pane-copy-changes',
  route: 'cockpit',
  select: PANE_CLAUDE,
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CLAUDE ? { ...p, cwd: 'C:\\Users\\dev\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c' } : p,
  ),
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.sleep(500);
  },
};
export default s;
