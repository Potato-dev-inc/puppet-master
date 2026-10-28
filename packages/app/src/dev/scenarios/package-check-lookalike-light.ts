import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';
import { LOOKALIKE_LIST, LOOKALIKE_PROMPT, LOOKALIKE_VIEW } from '../package-check-fixtures';
import type { HarnessScenario } from '../types';

// The look-alike install in the light theme: the Look closer glyph and weight carry the meaning, not colour.
const s: HarnessScenario = {
  name: 'package-check-lookalike-light',
  route: 'cockpit',
  theme: 'light',
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, status: 'waiting_input' } : p)),
  packageCheck: { list: LOOKALIKE_LIST, prompt: LOOKALIKE_PROMPT, lookup: LOOKALIKE_VIEW },
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
