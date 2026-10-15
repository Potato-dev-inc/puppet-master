import { QUEUE_PANES, ITEM_LOW } from '../attention-fixtures';
import { PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a different prompt is up. "This question changed. Look again." Nothing was typed.
const s: HarnessScenario = {
  name: 'phone-approval-changed',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CODEX,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_LOW], answer: 'changed' },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.clickText('button', 'Allow once');
    await dom.waitFor('[data-card-state="changed"]');
    await dom.sleep(300);
  },
};
export default s;
