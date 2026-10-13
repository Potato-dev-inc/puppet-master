import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'agent-doctor',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.sleep(500);
  },
};
export default s;
