import type { HarnessScenario } from '../types';

// The review sheet: skills and connection per agent, with the connection change expanded.
const s: HarnessScenario = {
  name: 'settings-agent-pack-preview',
  route: 'settings',
  settingsTab: 'bridge',
  after: async (dom) => {
    await dom.waitFor('[aria-label="Add Puppet Master to your agents"]');
    await dom.clickText('button', 'Review and install');
    await dom.waitFor('[role="dialog"] [data-testid="pack-agent"]');
    await dom.clickText('button', 'Show details');
    await dom.sleep(300);
  },
};
export default s;
