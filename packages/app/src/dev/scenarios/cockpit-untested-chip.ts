import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';
import { CLAUDE_UNTESTED_STATUS } from '../canary-fixtures';

// A pane header while its agent is on a version outside the tested range.
const s: HarnessScenario = {
  name: 'cockpit-untested-chip',
  route: 'cockpit',
  select: PANE_CLAUDE,
  canary: CLAUDE_UNTESTED_STATUS,
  after: async (dom) => {
    await dom.waitFor('[data-testid="untested-chip"]');
    await dom.sleep(300);
  },
};
export default s;
