import type { HarnessScenario } from '../types';

// Settings > Phone with a stable named tunnel; clicks "Test connection" to show the result.
const s: HarnessScenario = {
  name: 'settings-mobile',
  route: 'settings',
  settingsTab: 'phone',
  after: async (api) => {
    await api.waitFor('section[aria-label="Public address"]');
    await api.sleep(300);
    await api.clickText('button', 'Test connection');
    await api.sleep(400);
  },
};
export default s;
