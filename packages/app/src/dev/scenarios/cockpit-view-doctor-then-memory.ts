import type { HarnessScenario } from '../types';

// Live bug #8: with "Set up agents" open, clicking "Project" must show the memory view.
const s: HarnessScenario = {
  name: 'cockpit-view-doctor-then-memory',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('.ck-side-foot button', 'Project');
    await dom.waitFor('.ck-memory-file');
    await dom.sleep(500);
  },
};
export default s;
