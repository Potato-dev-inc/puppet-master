import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

const s: HarnessScenario = {
  name: 'loop-stuck',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.waitFor('.ck-stuck');
    await dom.sleep(400);
  },
};
export default s;
