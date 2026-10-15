import { QUEUE_PANES, tombstone, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: answered from another phone first.
const s: HarnessScenario = {
  name: 'phone-approval-answered-elsewhere',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [tombstone(ITEM_LOW, { state: 'answered', by: 'phone', choice_label: 'Denied', device_name: 'Anna\u2019s iPhone' })] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
