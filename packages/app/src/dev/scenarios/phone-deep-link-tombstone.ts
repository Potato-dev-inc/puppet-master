import { QUEUE_PANES, tombstone, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a push tap on an item that was already answered on the PC says so.
const s: HarnessScenario = {
  name: 'phone-deep-link-tombstone',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    search: 'item=a7Kp2xQ9',
    attention: { blocking: 0, items: [tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' })] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
