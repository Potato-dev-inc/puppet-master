import type { HarnessScenario } from '../types';

// Inline Settings in the light theme at 900px wide (General page).
const s: HarnessScenario = {
  name: 'cockpit-settings-light-900',
  route: 'cockpit',
  theme: 'light',
  window: { width: 900, height: 640 },
  after: async (dom) => {
    await dom.clickText('button', 'Settings');
    await dom.waitFor('.settings--inline');
    await dom.clickText('.st-nav-row', 'General');
    await dom.sleep(700);
  },
};
export default s;
