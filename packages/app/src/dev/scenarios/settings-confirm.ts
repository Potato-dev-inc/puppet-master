import type { HarnessScenario } from '../types';

// A destructive action asks first, in an in-app sheet (Keys > Remove all keys).
const s: HarnessScenario = {
  name: 'settings-confirm',
  route: 'settings',
  settingsTab: 'keys',
  after: async (dom) => {
    await dom.waitFor('[data-row="remove-all-keys"]');
    await dom.clickText('[data-row="remove-all-keys"] button', 'Remove');
    await dom.waitFor('[aria-modal="true"]');
    await dom.sleep(300);
  },
};
export default s;
