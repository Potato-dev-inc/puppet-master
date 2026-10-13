import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'cockpit-memory',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Project memory');
    await dom.waitFor('.ck-memory-file');
    await dom.sleep(600);
  },
};
export default s;
