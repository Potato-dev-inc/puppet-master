import type { HarnessScenario } from '../types';

// Settings > General scrolled to the bottom: storage, version, settings file, and the Uninstall group.
const s: HarnessScenario = {
  name: 'settings-general-more',
  route: 'settings',
  settingsTab: 'general',
  after: async (dom) => {
    await dom.waitFor('[data-row="uninstall"]');
    await dom.sleep(300);
    document.querySelector('.st-scroll')?.scrollTo({ top: 99999 });
  },
};
export default s;
