import { MISSION_TAMPER } from '../mission-fixtures';
import type { HarnessScenario } from '../types';

// The Inbox when a mission's checks pass but the tests were changed: the card blocks "done" until
// the person undoes the test changes or accepts them.
const s: HarnessScenario = {
  name: 'mission-tamper-card',
  route: 'cockpit',
  missions: { enabled: true, list: [MISSION_TAMPER] },
  after: async (dom) => {
    await dom.waitFor('[data-mission-card="tamper"]');
    await dom.sleep(400);
  },
};
export default s;
