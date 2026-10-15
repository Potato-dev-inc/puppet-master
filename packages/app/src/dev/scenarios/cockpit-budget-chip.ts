import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// A pane whose run has a budget: the header chip reads "12 of 20 turns" and its popover shows turns,
// time and the helper that draws from this budget. (The same pane without a budget has no chip.)
const s: HarnessScenario = {
  name: 'cockpit-budget-chip',
  route: 'cockpit',
  select: PANE_CLAUDE,
  budgets: {
    [PANE_CLAUDE]: {
      state: 'ok',
      depth: 0,
      is_helper: false,
      turns: { used: 12, own: 8, held_for_helpers: 4, limit: 20, add_step: 10 },
      minutes: { used: 14, limit: 60, add_step: 30 },
      time_up_in_turn: false,
      helpers: [{ agent: 'Codex', used: 2, limit: 4, closed: false }],
    },
  },
  after: async (dom) => {
    await dom.waitFor('[data-testid="budget-chip"]');
    await dom.click('[data-testid="budget-chip"]');
    await dom.waitFor('[data-testid="budget-popover"]');
    await dom.sleep(300);
  },
};
export default s;
