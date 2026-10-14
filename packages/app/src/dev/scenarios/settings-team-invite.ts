import { teamState } from '../team-fixtures';
import type { HarnessScenario } from '../types';

// "Invite someone": the QR code, the one-time code and the link, with how to send them.
const s: HarnessScenario = {
  name: 'settings-team-invite',
  route: 'settings',
  settingsTab: 'general',
  team: teamState(),
  after: async (dom) => {
    const row = await dom.waitFor('.st-team');
    row?.scrollIntoView({ block: 'center' });
    await dom.sleep(150);
    await dom.clickText('.st-team button', 'Invite someone');
    await dom.waitFor('.st-invite-code');
    await dom.sleep(300);
  },
};
export default s;
