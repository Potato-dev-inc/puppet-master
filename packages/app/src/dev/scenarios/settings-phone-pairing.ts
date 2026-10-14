import type { HarnessScenario } from '../types';

// Settings > Phone > Pair a phone: the choice of what a new phone may do, above the QR code.
const s: HarnessScenario = {
  name: 'settings-phone-pairing',
  route: 'settings',
  settingsTab: 'phone',
  window: { width: 1440, height: 1100 },
  after: async (api) => {
    await api.waitFor('[data-testid="pairing-panel"]');
    await api.sleep(400);
    document.querySelector('[data-testid="pairing-panel"]')?.scrollIntoView({ block: 'start' });
    await api.sleep(300);
  },
};
export default s;
