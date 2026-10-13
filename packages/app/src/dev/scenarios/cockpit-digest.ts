import type { HarnessScenario } from '../types';

// The calm full-pane digest shown after the user comes back (above the Inbox).
const s: HarnessScenario = {
  name: 'cockpit-digest',
  route: 'cockpit',
  awayDigest: true,
  after: async (dom) => {
    await dom.waitFor('.ck-away');
    await dom.sleep(600);
  },
};
export default s;
