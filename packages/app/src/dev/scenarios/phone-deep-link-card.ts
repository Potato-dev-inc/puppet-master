import { QUEUE_PANES, ITEM_HIGH } from '../attention-fixtures';
import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone: a push tap opens /?item=<id>: the agent with the card docked and its focus choice focused.
const s: HarnessScenario = {
  name: 'phone-deep-link-card',
  route: 'phone',
  window: { width: 390, height: 844 },
  phonePanes: QUEUE_PANES,
  phone: {
    activeTab: PANE_CLAUDE,
    digest: false,
    questions: false,
    search: 'item=h1ghR1sk',
    attention: { blocking: 0, items: [ITEM_HIGH] },
  },
  after: async (dom) => {
    await dom.waitFor('.ph-appr');
    await dom.sleep(700);
  },
};
export default s;
