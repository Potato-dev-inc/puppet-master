import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a phone that may only watch sees the card without buttons.
const s: HarnessScenario = {
  name: 'phone-approval-watch-only',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_LOW], device: { can_answer: false, can_type: false, can_control: false } },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
