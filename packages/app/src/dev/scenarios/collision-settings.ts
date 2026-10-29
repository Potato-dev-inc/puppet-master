import type { HarnessScenario } from '../types';

// Settings > Agents: the "Warn me when two agents change the same file" switch and its footer, under Same file.
const s: HarnessScenario = {
  name: 'collision-settings',
  route: 'settings',
  settingsTab: 'agents',
  after: async (dom) => {
    await dom.waitFor('[data-row="collision"]');
    await dom.sleep(300);
  },
};
export default s;
