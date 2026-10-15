import type { HarnessScenario } from '../types';

// Agents: Live updates moved to General > Labs; the page keeps a link where it used to be.
const s: HarnessScenario = {
  name: 'settings-agents-labs-link',
  route: 'settings',
  settingsTab: 'agents',
  window: { width: 1440, height: 1100 },
  after: async (dom) => {
    await dom.waitFor('[data-row="agents-live-updates-link"]');
    await dom.sleep(300);
  },
};
export default s;
