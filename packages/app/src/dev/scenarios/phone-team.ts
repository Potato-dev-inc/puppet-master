import type { HarnessScenario } from '../types';

// Phone: the Team group on the Agents screen, a phone that may act (control): Pull now / Queue my
// agent, Dismiss, and a finished update waiting to be applied.
const s: HarnessScenario = {
  name: 'phone-team',
  route: 'phone',
  window: { width: 390, height: 1000 },
  phone: { questions: false, digest: false, team: { can: { dismiss: true, act: true } } },
  after: async (dom) => {
    await dom.waitFor('[aria-label="Team"] .ph-team');
    await dom.waitFor('.ph-group .ph-row');
    await dom.sleep(500);
  },
};
export default s;
