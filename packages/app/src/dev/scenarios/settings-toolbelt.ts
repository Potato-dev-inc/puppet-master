import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'settings-toolbelt',
  route: 'settings',
  settingsTab: 'mcp',
  after: async (dom) => {
    await dom.waitFor('[aria-label="Toolbelt"] li');
    await dom.sleep(300);
  },
};
export default s;
