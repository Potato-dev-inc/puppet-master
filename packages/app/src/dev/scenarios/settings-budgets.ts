import type { HarnessScenario } from '../types';

// Settings > General > Budgets: the defaults. Both are 0 (off) until you set them.
const s: HarnessScenario = {
  name: 'settings-budgets',
  route: 'settings',
  settingsTab: 'general',
  window: { width: 1440, height: 1000 },
  after: async (dom) => {
    await dom.waitFor('[data-row="budget-turns"]');
    await dom.sleep(300);
    document.querySelector('[data-row="budget-turns"]')?.scrollIntoView({ block: 'center' });
    await dom.sleep(200);
  },
};
export default s;
