import type { HarnessScenario } from '../types';

const s: HarnessScenario = {
  name: 'panic-sheet',
  route: 'cockpit',
  after: async (dom) => {
    dom.key('k', { ctrl: true });
    await dom.sleep(300);
    await dom.clickText('.ck-palette-row', 'Stop all agents');
    await dom.waitFor('.ck-panic');
    await dom.sleep(1500);
  },
};
export default s;
