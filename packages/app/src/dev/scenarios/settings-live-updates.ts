import type { HarnessScenario } from '../types';

// Live updates is an early feature, listed in General > Labs. The deep link `live-updates` opens it there, scrolled
// to and held for a moment.
const s: HarnessScenario = {
  name: 'settings-live-updates',
  route: 'settings',
  settingsTab: 'live-updates',
  after: async (dom) => {
    await dom.waitFor('[data-row="live-updates"][data-target="true"]');
    await dom.sleep(300);
  },
};
export default s;
