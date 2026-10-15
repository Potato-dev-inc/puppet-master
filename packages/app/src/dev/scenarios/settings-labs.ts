import type { HarnessScenario } from '../types';

// General > Labs: every early feature in one place, with a one-line description each. Deep link `labs`.
const s: HarnessScenario = {
  name: 'settings-labs',
  route: 'settings',
  settingsTab: 'labs',
  window: { width: 1440, height: 1100 },
  after: async (dom) => {
    await dom.waitFor('[data-row="labs-index"][data-target="true"]');
    await dom.sleep(300);
  },
};
export default s;
