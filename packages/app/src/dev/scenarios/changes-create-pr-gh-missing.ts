import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// gh is not installed: plain words and the commands to copy, never a dead button.
const s: HarnessScenario = {
  name: 'changes-create-pr-gh-missing',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { prepared: { gh: 'missing' } },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.clickText('.ck-changes-actions button', 'Create PR');
    await dom.waitFor('.ck-pr-command');
    await dom.sleep(500);
  },
};
export default s;
