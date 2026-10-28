import { ALERTS } from '../collision-fixtures';
import type { HarnessScenario } from '../types';

// The same card in the light theme.
const s: HarnessScenario = {
  name: 'collision-card-light',
  route: 'cockpit',
  theme: 'light',
  collision: { alerts: ALERTS },
  after: async (dom) => {
    await dom.waitFor('[data-collision-id]');
    await dom.sleep(400);
  },
};
export default s;
