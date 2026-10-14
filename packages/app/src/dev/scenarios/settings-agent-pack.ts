import type { HarnessScenario } from '../types';

// Settings > Bridge & MCP: the "Add Puppet Master to your agents" row, under the Toolbelt.
const s: HarnessScenario = {
  name: 'settings-agent-pack',
  route: 'settings',
  settingsTab: 'bridge',
  window: { width: 1440, height: 1700 },
  after: async (dom) => {
    await dom.waitFor('[aria-label="Add Puppet Master to your agents"]');
    await dom.sleep(400);
  },
};
export default s;
