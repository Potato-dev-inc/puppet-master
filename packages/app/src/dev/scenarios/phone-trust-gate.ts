import { QUEUE_PANES, ITEM_TRUST } from '../attention-fixtures';
import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: folder trust. Don\u2019t trust leads, like Deny on a high-risk prompt.
const s: HarnessScenario = {
  name: 'phone-trust-gate',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CLAUDE,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_TRUST] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
