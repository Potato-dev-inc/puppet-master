import type { HarnessScenario } from '../types';

// Phone: the Agents list grouped Needs you / Working / Idle, with a question card and the away digest.
const s: HarnessScenario = {
  name: 'phone-agents',
  route: 'phone',
  window: { width: 390, height: 844 },
  after: async (dom) => {
    await dom.waitFor('[data-question-id]');
    await dom.waitFor('[aria-label="While you were away"]');
    await dom.waitFor('.ph-group');
    await dom.sleep(700);
  },
};
export default s;
