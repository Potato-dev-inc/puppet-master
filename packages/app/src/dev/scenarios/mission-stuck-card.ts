import { MISSION_STUCK, MISSION_WORKING } from '../mission-fixtures';
import type { HarnessScenario } from '../types';

// Every backup agent stopped making progress: the stuck card, and the other mission still working.
const s: HarnessScenario = {
  name: 'mission-stuck-card',
  route: 'cockpit',
  missions: { enabled: true, list: [MISSION_STUCK, MISSION_WORKING] },
  after: async (dom) => {
    await dom.waitFor('[data-mission-card="stuck"]');
    await dom.sleep(400);
  },
};
export default s;
