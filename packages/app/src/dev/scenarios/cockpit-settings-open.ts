import type { HarnessScenario } from '../types';

// Orchestration: the sidebar's Settings button. Before: nothing happens in this window (a separate Settings
// window opens). After: Settings fills the main area and the sidebar stays.
const s: HarnessScenario = {
  name: 'cockpit-settings-open',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Settings');
    await dom.sleep(900);
  },
};
export default s;
