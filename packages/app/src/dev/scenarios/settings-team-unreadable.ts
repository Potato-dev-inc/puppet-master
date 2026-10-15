import { teamConfig, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// team.json exists but couldn't be read: Team is off, and setting it up again replaces it.
const s: HarnessScenario = {
  name: 'settings-team-unreadable',
  route: 'settings',
  settingsTab: 'general',
  team: teamState({ enabled: false, active: false }),
  teamConfig: teamConfig({
    enabled: false,
    team: '',
    teamName: '',
    relay: '',
    members: [],
    joins: [],
    admin: false,
    transport: null,
    link: null,
    readError: 'expected value at line 1 column 1',
  }),
  after: async (dom) => {
    const row = await dom.waitFor('.st-team');
    row?.scrollIntoView({ block: 'center' });
    await dom.sleep(150);
  },
};
export default s;
