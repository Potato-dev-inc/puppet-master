import type { HarnessScenario } from '../types';

// "Set up your agents" with the Live updates line under each installed agent that can report.
const s: HarnessScenario = {
  name: 'agent-doctor-live-updates',
  route: 'cockpit',
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.sleep(500);
    document.querySelector('.ck-doctor-live')?.scrollIntoView({ block: 'center' });
  },
};
export default s;
