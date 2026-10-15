import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: the tap cannot leave the phone. "Can\u2019t reach your PC." Try again.
const s: HarnessScenario = {
  name: 'phone-approval-offline',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_LOW], answer: 'offline' },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.clickText('button', 'Allow once');
    await dom.waitFor('[data-card-state="offline"]');
    await dom.sleep(300);
  },
};
export default s;
