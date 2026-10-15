import type { HarnessScenario } from '../types';

// Phone: the Team group on a watch-only phone: the updates are readable, there are no buttons, and
// one line says where to change that.
const s: HarnessScenario = {
  name: 'phone-team-watch',
  route: 'phone',
  window: { width: 390, height: 1000 },
  phone: { questions: false, digest: false, team: { can: { dismiss: false, act: false }, missing: 2 } },
  after: async (dom) => {
    await dom.waitFor('[aria-label="Team"] .ph-team');
    await dom.waitFor('.ph-group .ph-row');
    await dom.sleep(500);
  },
};
export default s;
