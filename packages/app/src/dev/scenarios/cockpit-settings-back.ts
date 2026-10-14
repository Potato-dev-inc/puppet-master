import type { HarnessScenario } from '../types';

// Settings open, then Back: the view that was showing before (Set up agents) returns.
const s: HarnessScenario = {
  name: 'cockpit-settings-back',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Settings');
    await dom.waitFor('.settings--inline');
    await dom.click('button[aria-label="Back"]');
    await dom.sleep(600);
  },
};
export default s;
