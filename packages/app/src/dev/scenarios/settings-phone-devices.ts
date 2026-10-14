import type { HarnessScenario } from '../types';

// Settings > Phone > Devices: each paired phone's abilities as switches, its expiry and Revoke.
const s: HarnessScenario = {
  name: 'settings-phone-devices',
  route: 'settings',
  settingsTab: 'phone',
  window: { width: 1440, height: 1100 },
  after: async (api) => {
    await api.waitFor('[data-testid="pairing-panel"]');
    await api.sleep(400);
    const target =
      document.querySelector('[data-testid="paired-devices"]') ??
      document.querySelector('[data-testid="pairing-panel"]');
    target?.scrollIntoView({ block: 'start' });
    await api.sleep(300);
  },
};
export default s;
