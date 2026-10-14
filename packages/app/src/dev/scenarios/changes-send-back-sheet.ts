import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';
import { FAILING_CHECKS, PR } from '../ship-fixtures';

// "Send back" asks first and shows exactly what the agent will receive.
const s: HarnessScenario = {
  name: 'changes-send-back-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { worktree: { pr: { ...PR, checks: FAILING_CHECKS } }, checks: FAILING_CHECKS },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-pr');
    await dom.clickText('.ck-pr-actions button', 'Send back to');
    await dom.waitFor('#ck-sendback-t');
    await dom.click('.ck-pr-details summary');
    await dom.sleep(500);
  },
};
export default s;
