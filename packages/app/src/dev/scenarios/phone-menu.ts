import type { HarnessScenario } from '../types';

// Phone: the menu sheet with Notifications (iPhone, not installed: the Add to Home Screen hint) and
// Stop all agents. The iPhone user agent is spoofed only for this scenario.
const s: HarnessScenario = {
  name: 'phone-menu',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: {
    questions: false,
    digest: false,
    userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1',
  },
  after: async (dom) => {
    await dom.waitFor('.ph-group');
    await dom.click('[aria-label="Menu"]');
    await dom.waitFor('[role="dialog"] .ph-switch');
    await dom.sleep(800);
  },
};
export default s;
