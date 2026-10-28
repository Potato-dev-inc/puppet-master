import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The running stage: one status line, a thin bar and Stop. Find it starts the search.
const s: HarnessScenario = {
  name: 'what-broke-it-running',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.clickText('button', 'What broke the tests?');
    await dom.waitFor('.ck-sheet--wb');
    await dom.clickText('button', 'Find it');
    await dom.waitFor('.ck-wb-line');
    await dom.sleep(300);
  },
};
export default s;
