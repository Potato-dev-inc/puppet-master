import type { HarnessScenario } from '../types';
import { CHANGED_SCREENS_STATUS } from '../canary-fixtures';

// The Inbox after the version check found that an agent changed its screens.
const s: HarnessScenario = {
  name: 'cockpit-inbox-canary',
  route: 'cockpit',
  canary: CHANGED_SCREENS_STATUS,
  after: async (dom) => {
    await dom.waitFor('[data-testid="canary-notice"]');
    await dom.sleep(300);
  },
};
export default s;
