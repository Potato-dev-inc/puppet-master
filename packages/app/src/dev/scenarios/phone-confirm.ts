import type { HarnessScenario } from '../types';

// Phone: "Stop all agents" asks first, with an iOS-style action sheet over the menu.
const s: HarnessScenario = {
  name: 'phone-confirm',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { questions: false, digest: false },
  after: async (dom) => {
    await dom.waitFor('.ph-group');
    await dom.click('[aria-label="Menu"]');
    await dom.waitFor('[role="dialog"] .ph-switch');
    await dom.clickText('button', 'Stop all agents');
    await dom.waitFor('[role="alertdialog"]');
    await dom.sleep(600);
  },
};
export default s;
