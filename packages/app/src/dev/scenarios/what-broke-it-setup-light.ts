import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// The set up stage in light mode.
const s: HarnessScenario = {
  name: 'what-broke-it-setup-light',
  route: 'cockpit',
  select: PANE_CLAUDE,
  theme: 'light',
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.clickText('button', 'What broke the tests?');
    await dom.waitFor('.ck-sheet--wb');
    await dom.sleep(300);
  },
};
export default s;
