import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'settings-toolbelt-preview',
  route: 'settings',
  settingsTab: 'mcp',
  after: async (dom) => {
    await dom.waitFor('[aria-label="Toolbelt"] li');
    await dom.clickText('button', 'Apply to all agents');
    await dom.waitFor('[role="dialog"] [data-testid="toolbelt-host"]');
    await dom.clickText('button', 'Show file changes');
    await dom.sleep(300);
  },
};
export default s;
