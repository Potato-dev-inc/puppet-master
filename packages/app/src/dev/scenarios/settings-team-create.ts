import { teamConfig, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// Not on a team yet: create one, or join with an invite.
const s: HarnessScenario = {
  name: 'settings-team-create',
  route: 'settings',
  settingsTab: 'general',
  team: teamState({ enabled: false, active: false }),
  teamConfig: teamConfig({ enabled: false, team: '', teamName: '', relay: '', name: '', members: [], joins: [], admin: false, transport: null, link: null }),
  after: async (dom) => {
    const row = await dom.waitFor('.st-team');
    row?.scrollIntoView({ block: 'center' });
    await dom.sleep(150);
  },
};
export default s;
