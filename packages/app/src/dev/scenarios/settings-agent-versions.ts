import type { HarnessScenario } from '../types';
import { EXAMPLE_ACCOUNTS } from '../fixtures';
import { VERSIONS_STATUS } from '../canary-fixtures';

// Settings > Agents > Agent versions: Codex is on a version nobody tested (the chip and the plain
// sentence), Claude Code is a tested version, two unrecognised screens are saved.
const s: HarnessScenario = {
  name: 'settings-agent-versions',
  route: 'settings',
  settingsTab: 'agents',
  accounts: EXAMPLE_ACCOUNTS,
  canary: VERSIONS_STATUS,
  after: async (dom) => {
    const list = await dom.waitFor('[data-testid="agent-versions"]');
    list?.scrollIntoView({ block: 'center' });
    await dom.sleep(400);
  },
};
export default s;
