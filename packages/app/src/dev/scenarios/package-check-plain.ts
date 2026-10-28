import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { PLAIN_LIST, PLAIN_PROMPT, PLAIN_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// An ordinary install: plain facts only, no "Look closer" marker.
const s: HarnessScenario = {
  name: 'package-check-plain',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: PLAIN_LIST, prompt: PLAIN_PROMPT, lookup: PLAIN_VIEW },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
