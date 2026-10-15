import type { HarnessScenario } from '../types';

// Set up agents: Install for the missing agent (doctor-restart-needed).
const s: HarnessScenario = {
  name: 'doctor-restart-needed',
  route: 'cockpit',
  doctorInstall: { plan: 'winget', after: 'restart' },
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Install');
    await dom.waitFor('.ck-sheet--install');
    await dom.clickText('.ck-sheet-actions button', 'Run');
    await dom.waitFor('.ck-doctor-result', 6000);
    await dom.sleep(400);
    await dom.sleep(500);
  },
};
export default s;
