import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: still no word from the PC after eight seconds. "Still sending."
const s: HarnessScenario = {
  name: 'phone-approval-stuck-send',
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
    await dom.sleep(9200);
  },
};
export default s;
