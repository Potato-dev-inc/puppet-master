import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'cockpit-palette',
  route: 'cockpit',
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    await dom.sleep(800);
  },
};
export default s;
