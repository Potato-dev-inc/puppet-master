import type { HarnessScenario } from '../types';

// Phone: no agents are running but an agent's question is waiting. The question card shows once.
const s: HarnessScenario = {
  name: 'phone-agents-empty-questions',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { digest: false },
  phonePanes: [],
  after: async (dom) => {
    await dom.waitFor('[data-question-id]');
    await dom.sleep(600);
  },
};
export default s;
