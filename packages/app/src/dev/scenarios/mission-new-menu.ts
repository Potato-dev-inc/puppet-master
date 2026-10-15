import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// The "+" menu with Missions turned on in Labs: "Start a mission…" is the last item, tagged Preview.
const s: HarnessScenario = {
  name: 'mission-new-menu',
  route: 'cockpit',
  select: PANE_CLAUDE,
  missions: { enabled: true, list: [] },
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(800);
  },
};
export default s;
