import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';
import { FAILING_CHECKS, PR } from '../ship-fixtures';

// A draft PR whose GitHub checks failed: the failing names and the "Fix with <agent>" button (nothing is clicked).
const s: HarnessScenario = {
  name: 'changes-pr-checks-failed',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { worktree: { pr: { ...PR, checks: FAILING_CHECKS } }, checks: FAILING_CHECKS },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-pr');
    await dom.waitFor('.ck-pr-actions .ck-btn--primary');
    await dom.sleep(700);
  },
};
export default s;
