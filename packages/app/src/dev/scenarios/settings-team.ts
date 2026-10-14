import { teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// Settings > General > Labs > Team: Leo is the admin of Acme (relay, live), two others on the team and
// Priya waiting for a yes.
const s: HarnessScenario = {
  name: 'settings-team',
  route: 'settings',
  settingsTab: 'general',
  team: teamState(),
  after: async (dom) => {
    const row = await dom.waitFor('.st-team');
    row?.scrollIntoView({ block: 'center' });
    await dom.sleep(150);
  },
};
export default s;
