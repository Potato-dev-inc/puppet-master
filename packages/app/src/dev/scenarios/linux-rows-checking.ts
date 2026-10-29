import { EXAMPLE_PLACES } from '../wsl-fixtures';
import type { HarnessScenario } from '../types';

// Settings > Agents > Run agents in Linux while "Check Ubuntu-Test" is still running (the check never answers here).
const s: HarnessScenario = {
  name: 'linux-rows-checking',
  route: 'settings',
  settingsTab: 'agents',
  window: { width: 1440, height: 1300 },
  wsl: {
    status: { state: 'ready', distros: [{ name: 'Ubuntu-Test', default: true, version: 2, state: 'Running' }] },
    places: EXAMPLE_PLACES,
    check: 'hang',
  },
  after: async (dom) => {
    await dom.waitFor('[data-row="linux"]');
    document.querySelector('[data-row="linux"]')?.scrollIntoView({ block: 'start' });
    await dom.clickText('button', 'Check Ubuntu-Test');
    await dom.sleep(500);
  },
};
export default s;
