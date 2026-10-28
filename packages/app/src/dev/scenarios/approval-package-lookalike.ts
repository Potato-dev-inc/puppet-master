import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { LOOKALIKE_LIST, LOOKALIKE_PROMPT, LOOKALIKE_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// The full approval card for `npm install lodahs`: High risk, Deny first, the package row on the card, and Details open with the facts.
const s: HarnessScenario = {
  name: 'approval-package-lookalike',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: LOOKALIKE_LIST, prompt: LOOKALIKE_PROMPT, lookup: LOOKALIKE_VIEW },
  after: async (dom) => {
    await dom.sleep(800);
    await dom.clickText('button', 'Details');
    await dom.sleep(200);
  },
};
export default s;
