import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// The same sheet in the light theme.
const s: HarnessScenario = {
  name: 'keep-going-sheet-light',
  route: 'cockpit',
  theme: 'light',
  questions: [LIMIT_CARD],
  after: async (dom) => {
    await dom.clickText('button', 'Other agent');
    await dom.waitFor('.ck-keep-going-sheet');
    await dom.sleep(500);
  },
};
export default s;
