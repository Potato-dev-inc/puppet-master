import { PANE_CLAUDE } from '../fixtures';
import type { HarnessScenario } from '../types';

// Phone paired with "Watch and answer only": the agent's terminal shows, with a plain line in place of the input box.
const s: HarnessScenario = {
  name: 'phone-watch-only',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { activeTab: PANE_CLAUDE },
  phoneCaps: ['watch', 'answer'],
  after: async (dom) => {
    await dom.waitFor('.xterm');
    await dom.sleep(1500);
  },
};
export default s;
