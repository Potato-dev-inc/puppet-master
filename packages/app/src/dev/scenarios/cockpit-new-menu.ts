import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'cockpit-new-menu',
  route: 'cockpit',
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(800);
  },
};
export default s;
