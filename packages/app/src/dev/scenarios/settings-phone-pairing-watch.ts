import type { HarnessScenario } from '../types';

// Settings > Phone > Pair a phone with "Watch and answer only" chosen (a new code is made for it).
const s: HarnessScenario = {
  name: 'settings-phone-pairing-watch',
  route: 'settings',
  settingsTab: 'phone',
  window: { width: 1440, height: 1100 },
  after: async (api) => {
    await api.waitFor('[data-testid="pairing-choice"]');
    await api.sleep(400);
    await api.click('input[value="watch_answer"]');
    await api.sleep(400);
    document.querySelector('[data-testid="pairing-panel"]')?.scrollIntoView({ block: 'start' });
    await api.sleep(300);
  },
};
export default s;
