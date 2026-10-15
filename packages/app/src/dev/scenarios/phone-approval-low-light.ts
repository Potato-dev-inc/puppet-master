import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: the low-risk approval, light theme.
const s: HarnessScenario = {
  name: 'phone-approval-low-light',
  route: 'phone',
  window: { width: 390, height: 844 },
  theme: 'light',
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_LOW] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
