import type { HarnessScenario } from '../types';

// Set up agents: Install for the missing agent (doctor-install-script).
const s: HarnessScenario = {
  name: 'doctor-install-script',
  route: 'cockpit',
  doctorInstall: { plan: 'script' },
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Install');
    await dom.waitFor('.ck-install-risk');
    await dom.sleep(500);
  },
};
export default s;
