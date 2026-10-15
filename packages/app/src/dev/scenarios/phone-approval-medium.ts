import { QUEUE_PANES, ITEM_MEDIUM } from '../attention-fixtures';
import { PANE_OPENCODE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a Check first approval (no lock-screen buttons, full buttons in the card).
const s: HarnessScenario = {
  name: 'phone-approval-medium',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_OPENCODE,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_MEDIUM] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
