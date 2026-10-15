import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_PWSH } from '../fixtures';

// A pane that works in a separate copy: the sidebar names its project ("shop"), never the id of the copy's folder.
const COPY = 'C:\\Users\\dev\\work\\shop\\.puppet-master\\worktrees\\7d4cb2b3-2-209c';
const s: HarnessScenario = {
  name: 'shell-copy-pane',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_PWSH ? { ...p, cwd: COPY } : p)),
  after: async (dom) => {
    await dom.sleep(600);
  },
};
export default s;
