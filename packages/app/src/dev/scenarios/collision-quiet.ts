import { QUIET_ALERTS } from '../collision-fixtures';
import type { HarnessScenario } from '../types';

// Collision Alert, quiet level: the same file in two copies, far apart lines. One muted "No clash yet" row with
// "Ignore" and the footer; no Open buttons, no banner in the chats.
const s: HarnessScenario = {
  name: 'collision-quiet',
  route: 'cockpit',
  collision: { alerts: QUIET_ALERTS },
  after: async (dom) => {
    await dom.waitFor('[data-collision-id]');
    await dom.sleep(400);
  },
};
export default s;
