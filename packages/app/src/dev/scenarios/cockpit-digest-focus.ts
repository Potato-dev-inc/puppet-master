import type { HarnessScenario } from '../types';

// The calm full-pane layer shown when a Focus ends: "While you focused" (above the Inbox).
const s: HarnessScenario = {
  name: 'cockpit-digest-focus',
  route: 'cockpit',
  awayFocus: true,
  after: async (dom) => {
    await dom.waitFor('.ck-away');
    await dom.sleep(600);
  },
};
export default s;
