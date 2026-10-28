import type { HarnessScenario } from '../types';

// Settings > Agents > Run agents in Linux when WSL is not installed: the official command with Copy and Check again.
const s: HarnessScenario = {
  name: 'linux-rows-missing',
  route: 'settings',
  settingsTab: 'agents',
  window: { width: 1440, height: 1300 },
  wsl: { status: { state: 'not_installed', command: 'wsl --install' } },
  after: async (dom) => {
    await dom.waitFor('[data-row="linux"]');
    document.querySelector('[data-row="linux"]')?.scrollIntoView({ block: 'start' });
    await dom.sleep(300);
  },
};
export default s;
