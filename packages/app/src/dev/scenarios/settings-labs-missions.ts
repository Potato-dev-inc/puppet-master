import type { HarnessScenario } from '../types';

// Settings > General > Labs: the Missions (Preview) switch, off by default.
const s: HarnessScenario = {
  name: 'settings-labs-missions',
  route: 'settings',
  settingsTab: 'general',
  after: async (dom) => {
    await dom.sleep(400);
    const rows = [...document.querySelectorAll('*')].filter((el) => el.children.length === 0 && el.textContent?.trim() === 'Missions (Preview)');
    rows[0]?.scrollIntoView({ block: 'center' });
    await dom.sleep(200);
  },
};
export default s;
