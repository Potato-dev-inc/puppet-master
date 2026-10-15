import { QUEUE_PANES, ITEM_HIGH } from '../attention-fixtures';
import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a high-risk approval. Deny leads and has the focus; Allow once needs a deliberate tap.
const s: HarnessScenario = {
  name: 'phone-approval-high',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CLAUDE,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_HIGH] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
