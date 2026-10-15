import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: the tap is on its way. "Sending to your PC", buttons off.
const s: HarnessScenario = {
  name: 'phone-approval-sending',
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
    await dom.sleep(2200);
  },
};
export default s;
