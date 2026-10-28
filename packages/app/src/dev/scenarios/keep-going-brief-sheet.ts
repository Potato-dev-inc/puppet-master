import { PANE_CODEX } from '../fixtures';
import { RUNNING_LINEAGE } from '../keep-going-fixtures';
import type { HarnessScenario } from '../types';

// "Show brief" on the new pane: the sheet with the text Codex got, its size and Copy / Done.
const s: HarnessScenario = {
  name: 'keep-going-brief-sheet',
  route: 'cockpit',
  select: PANE_CODEX,
  keepGoing: { lineage: RUNNING_LINEAGE },
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Chat');
    await dom.clickText('button', 'Show brief');
    await dom.waitFor('[role="dialog"] pre');
    await dom.sleep(500);
  },
};
export default s;
