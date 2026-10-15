import { QUEUE_PANES, tombstone, ITEM_HIGH } from '../attention-fixtures';
import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: the prompt went away while the card was open. "This question has gone."
const s: HarnessScenario = {
  name: 'phone-approval-gone',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CLAUDE,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [tombstone(ITEM_HIGH, { state: 'gone', reason: 'moved_on' })] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
