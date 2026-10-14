import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = {
  name: 'cockpit-changes-merge',
  route: 'cockpit',
  select: PANE_CLAUDE,
  theme: 'light',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.clickText('.ck-changes-actions button', 'Merge');
    await dom.sleep(800);
  },
};
export default s;
