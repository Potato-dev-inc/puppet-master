import type { HarnessScenario } from '../types';

// Phone: "Queue my agent" asks first, with an iOS-style action sheet over the Team group.
const s: HarnessScenario = {
  name: 'phone-team-queue',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { questions: false, digest: false, team: {} },
  after: async (dom) => {
    await dom.waitFor('[aria-label="Team"] .ph-team');
    await dom.clickText('button', 'Queue my agent');
    await dom.waitFor('[role="alertdialog"]');
    await dom.sleep(600);
  },
};
export default s;
