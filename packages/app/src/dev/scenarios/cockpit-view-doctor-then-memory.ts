import type { HarnessScenario } from '../types';

// Live bug #8: with "Set up agents" open, clicking "Project memory" must show the memory view.
const s: HarnessScenario = {
  name: 'cockpit-view-doctor-then-memory',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Project memory');
    await dom.waitFor('.ck-memory-file');
    await dom.sleep(500);
  },
};
export default s;
