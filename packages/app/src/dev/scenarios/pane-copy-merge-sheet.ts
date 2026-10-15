import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';

// The merge sheet of a separate copy: plain words, no branch name.
const s: HarnessScenario = {
  name: 'pane-copy-merge-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CLAUDE ? { ...p, cwd: 'C:\\Users\\dev\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c' } : p,
  ),
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-actions');
    await dom.clickText('.ck-changes-actions button', 'Merge');
    await dom.waitFor('.ck-sheet');
    await dom.sleep(400);
  },
};
export default s;
