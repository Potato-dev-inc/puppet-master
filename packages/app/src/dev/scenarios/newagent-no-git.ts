import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The "+" menu for a project that does not use Git: Separate copy is off, with a one-line hint and "Set up Git here".
const s: HarnessScenario = {
  name: 'newagent-no-git',
  route: 'cockpit',
  select: PANE_CLAUDE,
  ship: { gitState: 'folder' },
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.waitFor('.ck-menu-note--row');
    await dom.sleep(500);
  },
};
export default s;
