import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { LOOKALIKE_LIST, LOOKALIKE_PROMPT, OFF_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// Settings > General > Safety > Check packages before installs is off: no lookups, the look-alike still shows.
const s: HarnessScenario = {
  name: 'package-check-off',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: LOOKALIKE_LIST, prompt: LOOKALIKE_PROMPT, lookup: OFF_VIEW, lookups: false },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
