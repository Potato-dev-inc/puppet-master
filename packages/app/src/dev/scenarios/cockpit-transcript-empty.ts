import type { HarnessScenario } from '../types';
import { PANE_CODEX } from '../fixtures';

// A pane that was not started by a run: Transcript and Receipt say so in plain words.
const s: HarnessScenario = {
  name: 'cockpit-transcript-empty',
  route: 'cockpit',
  select: PANE_CODEX,
  after: async (dom) => {
    await dom.clickText('[role="tab"]', 'Transcript');
    await dom.sleep(1200);
  },
};
export default s;
