import type { HarnessScenario } from '../types';

// Phone: the "Open an agent on your PC" sheet.
const s: HarnessScenario = {
  name: 'phone-new-agent',
  route: 'phone',
  window: { width: 390, height: 844 },
  after: async (dom) => {
    await dom.waitFor('.ph-group');
    await dom.click('[aria-label="Open a new agent"]');
    await dom.waitFor('[role="dialog"] .ph-row');
    await dom.sleep(600);
  },
};
export default s;
