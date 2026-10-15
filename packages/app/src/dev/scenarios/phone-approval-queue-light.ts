import { ITEM_HIGH, ITEM_LOW, ITEM_QUESTION, QUEUE_PANES } from '../attention-fixtures';
import type { HarnessScenario } from '../types';

// Phone: the Needs you queue. Three things wait, in the PC's order: the high-risk approval first
// (Deny leads), the calm one, then the agent's question. The counter is the PC's number.
const s: HarnessScenario = {
  name: 'phone-approval-queue-light',
  route: 'phone',
  window: { width: 390, height: 844 },
  theme: 'light',
  phonePanes: QUEUE_PANES,
  phone: {
    digest: false,
    attention: { blocking: 3, items: [ITEM_HIGH, ITEM_LOW, ITEM_QUESTION] },
  },
  after: async (dom) => {
    await dom.waitFor('[data-testid="needs-you-queue"]');
    await dom.sleep(700);
  },
};
export default s;
