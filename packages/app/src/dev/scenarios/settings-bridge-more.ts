import type { HarnessScenario } from '../types';

// Settings > Bridge & MCP scrolled past the Toolbelt: runtime status, install, snippets, Uninstall MCP.
const s: HarnessScenario = {
  name: 'settings-bridge-more',
  route: 'settings',
  settingsTab: 'bridge',
  after: async (dom) => {
    await dom.waitFor('[data-row="mcp-uninstall"]');
    await dom.sleep(400);
    document.querySelector('.st-scroll')?.scrollTo({ top: 99999 });
  },
};
export default s;
