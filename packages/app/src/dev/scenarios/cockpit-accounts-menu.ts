import type { HarnessScenario } from '../types';
import { EXAMPLE_ACCOUNTS } from '../fixtures';

// The "+" menu: Claude Code has two accounts, so choosing it opens the login step.
const s: HarnessScenario = {
  name: 'cockpit-accounts-menu',
  route: 'cockpit',
  accounts: EXAMPLE_ACCOUNTS,
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(500);
    await dom.clickText('.ck-menu-item', 'Claude');
    await dom.sleep(1500);
  },
};
export default s;
