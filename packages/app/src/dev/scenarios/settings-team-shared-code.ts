import { teamConfig, teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

const WARNING = "Someone else also used this invite's code. Only let in the person you invited.";
const base = teamConfig();

// Settings > Team: two people answered one invite with the right code, so both requests are flagged;
// Sam's GitHub login is only claimed and the admin can confirm it (security review L5, L8).
const s: HarnessScenario = {
  name: 'settings-team-shared-code',
  route: 'settings',
  settingsTab: 'general',
  team: teamState(),
  teamConfig: {
    joins: [
      ...base.joins.map((j) => ({ ...j, warning: WARNING })),
      { ...base.joins[0], id: 'j-eve', name: 'Eve', github: null, device: 'Desktop', warning: WARNING },
    ],
  },
  after: async (dom) => {
    const row = await dom.waitFor('.st-team');
    row?.scrollIntoView({ block: 'center' });
    await dom.sleep(150);
  },
};
export default s;
