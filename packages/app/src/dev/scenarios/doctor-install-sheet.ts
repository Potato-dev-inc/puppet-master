import type { HarnessScenario } from '../types';

// Set up agents: Install for the missing agent (doctor-install-sheet).
const s: HarnessScenario = {
  name: 'doctor-install-sheet',
  route: 'cockpit',
  doctorInstall: { plan: 'winget' },
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Install');
    await dom.waitFor('.ck-sheet--install');
    await dom.sleep(500);
  },
};
export default s;
