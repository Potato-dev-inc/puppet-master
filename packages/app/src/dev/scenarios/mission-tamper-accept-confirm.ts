import { MISSION_TAMPER } from '../mission-fixtures';
import type { HarnessScenario } from '../types';

// "Accept the changes" asks first: "Count these tests as passing?"
const s: HarnessScenario = {
  name: 'mission-tamper-accept-confirm',
  route: 'cockpit',
  missions: { enabled: true, list: [MISSION_TAMPER] },
  after: async (dom) => {
    await dom.waitFor('[data-mission-card="tamper"]');
    await dom.clickText('button', 'Accept the changes');
    await dom.waitFor('#ck-mission-accept-t');
    await dom.sleep(400);
  },
};
export default s;
