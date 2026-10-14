import type { HarnessScenario } from '../types';

// Agents tab, scrolled to the Live updates setting.
const s: HarnessScenario = {
  name: 'settings-live-updates',
  route: 'settings',
  settingsTab: 'agents',
  after: async (dom) => {
    await dom.sleep(300);
    const label = [...document.querySelectorAll('*')].find((e) => e.children.length === 0 && e.textContent?.trim() === 'Live updates');
    label?.scrollIntoView({ block: 'center' });
  },
};
export default s;
