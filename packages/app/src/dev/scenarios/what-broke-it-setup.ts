import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The set up stage: the declared `npm test`, the save point list and the limits. Opened from History.
const s: HarnessScenario = {
  name: 'what-broke-it-setup',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.clickText('button', 'What broke the tests?');
    await dom.waitFor('.ck-sheet--wb');
    await dom.sleep(300);
  },
};
export default s;
