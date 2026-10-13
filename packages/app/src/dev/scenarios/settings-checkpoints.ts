import type { HarnessScenario } from '../types';

// General tab, scrolled to the Checkpoints settings (the loop-guard settings sit above them).
const s: HarnessScenario = {
  name: 'settings-checkpoints',
  route: 'settings',
  settingsTab: 'general',
  after: async (dom) => {
    await dom.sleep(300);
    const label = [...document.querySelectorAll('*')].find((e) => e.children.length === 0 && e.textContent?.trim() === 'Checkpoints');
    label?.scrollIntoView({ block: 'center' });
  },
};
export default s;
