import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: no word for twenty seconds. "Didn\u2019t hear back." Try again sends the same answer key.
const s: HarnessScenario = {
  name: 'phone-approval-lost',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_LOW], answer: 'hang' },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.clickText('button', 'Allow once');
    await dom.waitFor('[data-card-state="lost"]', 30000);
    await dom.sleep(300);
  },
};
export default s;
