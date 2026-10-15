import type { HarnessScenario } from '../types';

// General > About > What's new in 0.2.0, reached by the deep link `whats-new`.
const s: HarnessScenario = {
  name: 'settings-whats-new',
  route: 'settings',
  settingsTab: 'whats-new',
  after: async (dom) => {
    await dom.waitFor('[data-row="whats-new"][data-target="true"]');
    await dom.sleep(300);
  },
};
export default s;
