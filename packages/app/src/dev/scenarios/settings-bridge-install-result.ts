import type { HarnessScenario } from '../types';

// Settings > Bridge & MCP after "Install globally": each result names the agent and where, not an internal id.
const s: HarnessScenario = {
  name: 'settings-bridge-install-result',
  route: 'settings',
  settingsTab: 'bridge',
  window: { width: 1440, height: 1000 },
  after: async (dom) => {
    await dom.waitFor('[data-row="mcp-install"]');
    await dom.clickText('button', 'Install globally');
    await dom.sleep(600);
    document.querySelector('[data-row="mcp-install"]')?.scrollIntoView({ block: 'start' });
    await dom.sleep(300);
  },
};
export default s;
