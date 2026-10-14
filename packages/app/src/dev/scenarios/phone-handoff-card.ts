import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// Phone: the usage-limit card among the agents. Same two choices; no waiting caption (it goes away with the limit).
const s: HarnessScenario = {
  name: 'phone-handoff-card',
  route: 'phone',
  window: { width: 390, height: 844 },
  phone: { questionList: [LIMIT_CARD], digest: false },
  after: async (dom) => {
    await dom.waitFor('.ph-card');
    await dom.sleep(600);
  },
};
export default s;
