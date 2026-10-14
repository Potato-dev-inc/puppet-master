import type { HarnessScenario } from '../types';

// Set up agents: Install for the missing agent (doctor-installing).
const s: HarnessScenario = {
  name: 'doctor-installing',
  route: 'cockpit',
  doctorInstall: { plan: 'winget', after: 'running' },
  after: async (dom) => {
    await dom.clickText('button', 'Set up agents');
    await dom.waitFor('.ck-doctor-row');
    await dom.clickText('button', 'Install');
    await dom.waitFor('.ck-sheet--install');
    await dom.clickText('.ck-sheet-actions button', 'Run');
    await dom.waitFor('[data-state="not_installed"] [aria-label]');
    await dom.sleep(300);
    await dom.sleep(500);
  },
};
export default s;
