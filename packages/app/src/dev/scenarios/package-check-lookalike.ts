import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { LOOKALIKE_LIST, LOOKALIKE_PROMPT, LOOKALIKE_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// `npm install lodahs`: High risk (Deny first). The look-alike row is there at once; the registry facts fill the same row.
const s: HarnessScenario = {
  name: 'package-check-lookalike',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: LOOKALIKE_LIST, prompt: LOOKALIKE_PROMPT, lookup: LOOKALIKE_VIEW },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
