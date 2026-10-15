import type { HarnessScenario } from '../types';

// Settings > Phone > Pair a phone, after the person pressed "Pair a phone": the code and QR show.
// (settings-phone-pairing shows the panel as it opens: no code yet.)
const s: HarnessScenario = {
  name: 'settings-phone-pair-minted',
  route: 'settings',
  settingsTab: 'phone',
  window: { width: 1440, height: 1100 },
  after: async (api) => {
    await api.waitFor('[data-testid="pairing-panel"]');
    await api.sleep(300);
    await api.click('[data-testid="pair-a-phone"]');
    await api.sleep(700);
    document.querySelector('[data-testid="pairing-panel"]')?.scrollIntoView({ block: 'start' });
    await api.sleep(300);
  },
};
export default s;
