import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// Pane > Transcript with the real stream tags (user_task, assistant_final, ...): labels must be plain words.
const s: HarnessScenario = {
  name: 'cockpit-transcript',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Transcript');
    await dom.sleep(1500);
  },
};
export default s;
