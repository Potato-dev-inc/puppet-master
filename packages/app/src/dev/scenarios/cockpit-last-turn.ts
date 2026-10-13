import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// Receipt tab with the "Last turn" sentence on top (turn summary, no LLM).
const s: HarnessScenario = {
  name: 'cockpit-last-turn',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.sleep(500);
    await dom.waitFor('.ck-lastturn');
    await dom.sleep(800);
  },
};
export default s;
