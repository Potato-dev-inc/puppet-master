import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'askbar-empty',
  route: 'askbar',
  after: async (dom) => {
    await dom.waitFor('.askbar-input');
    await dom.sleep(300);
  },
};
export default s;
