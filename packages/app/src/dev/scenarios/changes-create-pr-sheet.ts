import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// Create a draft pull request: title and body made from the receipt.
const s: HarnessScenario = {
  name: 'changes-create-pr-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.clickText('.ck-changes-actions button', 'Create PR');
    await dom.waitFor('.ck-pr-body');
    await dom.sleep(500);
  },
};
export default s;
