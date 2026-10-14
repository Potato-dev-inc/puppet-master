import type { HarnessScenario } from '../types';

// Phone: the PC says five things need you (two panes, a question, a stuck pane and a consent card),
// while this phone can only see three of them. The counter shows the PC's number, the same one the
// desktop Inbox, the tray badge and the away digest show.
const s: HarnessScenario = {
  name: 'phone-attention-count',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { digest: false, attention: { blocking: 5, ready: 1 } },
  after: async (dom) => {
    await dom.waitFor('.ph-group');
    await dom.sleep(700);
  },
};
export default s;
