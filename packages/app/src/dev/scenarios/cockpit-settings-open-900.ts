import type { HarnessScenario } from '../types';

// Same as cockpit-settings-open at 900px wide (the cockpit's smallest window).
const s: HarnessScenario = {
  name: 'cockpit-settings-open-900',
  route: 'cockpit',
  window: { width: 900, height: 640 },
  after: async (dom) => {
    await dom.clickText('button', 'Settings');
    await dom.sleep(900);
  },
};
export default s;
