import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// At 80% or more the chip goes bold (no colour, no push, no card).
const s: HarnessScenario = {
  name: 'cockpit-budget-nearly',
  route: 'cockpit',
  select: PANE_CLAUDE,
  budgets: {
    [PANE_CLAUDE]: {
      state: 'nearly',
      depth: 0,
      is_helper: false,
      turns: { used: 17, own: 17, held_for_helpers: 0, limit: 20, add_step: 10 },
      minutes: null,
      time_up_in_turn: false,
      helpers: [],
    },
  },
  after: async (dom) => {
    await dom.waitFor('[data-testid="budget-chip"]');
    await dom.sleep(300);
  },
};
export default s;
