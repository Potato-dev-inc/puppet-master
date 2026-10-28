import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { MANY_LIST, MANY_PROMPT, MANY_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// Seven packages from five ecosystems: three rows on the card, the rest and the secret-looking name in Details.
const s: HarnessScenario = {
  name: 'package-check-many',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: MANY_LIST, prompt: MANY_PROMPT, lookup: MANY_VIEW },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
