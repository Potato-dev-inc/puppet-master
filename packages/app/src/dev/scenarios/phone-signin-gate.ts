import { QUEUE_PANES, ITEM_SIGNIN } from '../attention-fixtures';
import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a sign-in screen has nothing to choose. Only the PC can pass it.
const s: HarnessScenario = {
  name: 'phone-signin-gate',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CLAUDE,
    digest: false,
    questions: false,
    attention: { blocking: 0, items: [ITEM_SIGNIN] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(500);
  },
};
export default s;
