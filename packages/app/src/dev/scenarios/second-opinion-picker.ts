import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// Receipt tab with the "Second opinion" picker sheet open.
const s: HarnessScenario = {
  name: 'second-opinion-picker',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Receipt');
    await dom.waitFor('button');
    await dom.clickText('button', 'Second opinion');
    await dom.waitFor('.ck-sheet');
    await dom.sleep(400);
  },
};
export default s;
