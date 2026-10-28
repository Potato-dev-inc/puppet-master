import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// "Other agent…" on the limit card: Codex is Ready, the other two rows say why they cannot take over, and the
// move-back switch is there because the card knows when the limit ends.
const s: HarnessScenario = {
  name: 'keep-going-sheet',
  route: 'cockpit',
  questions: [LIMIT_CARD],
  after: async (dom) => {
    await dom.clickText('button', 'Other agent');
    await dom.waitFor('.ck-keep-going-sheet');
    await dom.sleep(500);
  },
};
export default s;
