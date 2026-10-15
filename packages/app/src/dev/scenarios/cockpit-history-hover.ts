import type { HarnessScenario } from '../types';
import { PANE_CLAUDE } from '../fixtures';

// History: Restore shows on hover or keyboard focus (focused here), one primary "Undo last turn" on top.
const s: HarnessScenario = {
  name: 'cockpit-history-hover',
  route: 'cockpit',
  select: PANE_CLAUDE,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'History');
    await dom.waitFor('.ck-hist-item');
    document.querySelectorAll<HTMLButtonElement>('.ck-hist-restore')[1]?.focus();
    await dom.sleep(400);
  },
};
export default s;
