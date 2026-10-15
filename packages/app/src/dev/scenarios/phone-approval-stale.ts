import { QUEUE_PANES, tombstone, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: answered on the PC first. The card says so and has no buttons.
const s: HarnessScenario = {
  name: 'phone-approval-stale',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [tombstone(ITEM_LOW, { state: 'answered', by: 'desktop', choice_label: 'Allowed once' })] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
