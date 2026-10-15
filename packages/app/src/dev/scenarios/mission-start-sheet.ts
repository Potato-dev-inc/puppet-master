import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// "+" → Start a mission… with the goal typed in: the checks this project runs, who works on it
// (one at a time), the budget and where. Start is enabled.
const s: HarnessScenario = {
  name: 'mission-start-sheet',
  route: 'cockpit',
  select: PANE_CLAUDE,
  window: { width: 1440, height: 1000 },
  missions: { enabled: true, list: [] },
  after: async (dom) => {
    await dom.click('button[aria-label="New agent"]');
    await dom.sleep(300);
    await dom.clickText('.ck-menu-item', 'Start a mission');
    const goal = (await dom.waitFor('#ck-mission-goal')) as HTMLTextAreaElement | null;
    if (goal) {
      const set = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      set?.call(goal, 'Make the cart tests pass and round totals to cents.');
      goal.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await dom.sleep(500);
  },
};
export default s;
