import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { LOOKALIKE_LIST, LOOKALIKE_PROMPT } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// No network: the look-alike row stays from the first paint and the header says "Could not check".
const s: HarnessScenario = {
  name: 'package-check-offline',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: LOOKALIKE_LIST, prompt: LOOKALIKE_PROMPT, lookup: 'error' },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
