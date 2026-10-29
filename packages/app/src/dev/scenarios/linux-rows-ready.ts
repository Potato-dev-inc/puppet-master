import { EXAMPLE_CHECK, EXAMPLE_PLACES } from '../wsl-fixtures';
import type { HarnessScenario } from '../types';

// Settings > Agents > Run agents in Linux with WSL ready: a WSL 2 distro (default), a WSL 1 one (no Check),
// Claude Code set to Linux and not checked yet. "Check Ubuntu-Test" runs, then the found line shows.
const s: HarnessScenario = {
  name: 'linux-rows-ready',
  route: 'settings',
  settingsTab: 'agents',
  window: { width: 1440, height: 1300 },
  wsl: {
    status: {
      state: 'ready',
      distros: [
        { name: 'Ubuntu-Test', default: true, version: 2, state: 'Running' },
        { name: 'Debian-Test', default: false, version: 1, state: 'Stopped' },
      ],
    },
    places: EXAMPLE_PLACES,
    check: EXAMPLE_CHECK,
  },
  after: async (dom) => {
    await dom.waitFor('[data-row="linux"]');
    document.querySelector('[data-row="linux"]')?.scrollIntoView({ block: 'start' });
    await dom.clickText('button', 'Check Ubuntu-Test');
    await dom.sleep(800);
  },
};
export default s;
