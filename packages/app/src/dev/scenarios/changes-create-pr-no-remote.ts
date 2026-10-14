import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The project has no GitHub remote: plain words, and the commands to copy for when one is added.
const s: HarnessScenario = {
  name: 'changes-create-pr-no-remote',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { prepared: { has_remote: false, on_github: false } },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.clickText('.ck-changes-actions button', 'Create PR');
    await dom.waitFor('.ck-sheet--pr');
    await dom.sleep(500);
  },
};
export default s;
