import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// Changes tab, a review started from the picker; the answer lands and Changes opens the first file with notes.
const s: HarnessScenario = {
  name: 'second-opinion-changes',
  route: 'cockpit',
  window: { width: 1280, height: 1100 },
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Changes');
    await dom.waitFor('.ck-changes-file');
    await dom.clickText('button', 'Second opinion');
    await dom.waitFor('.ck-sheet');
    await dom.waitFor('.ck-so-row');
    await dom.clickText('button', 'Start review');
    await dom.waitFor('.ck-find', 8000);
    await dom.sleep(600);
  },
};
export default s;
