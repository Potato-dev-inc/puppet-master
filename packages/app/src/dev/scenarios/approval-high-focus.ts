import { EXAMPLE_PANES, PANE_CLAUDE, PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// One high-risk approval, as it first appears: keyboard focus sits on Deny and the ring shows where Enter goes.
const s: HarnessScenario = {
  name: 'approval-high-focus',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CLAUDE ? { ...p, status: 'idle' } : p.id === PANE_CODEX ? { ...p, status: 'waiting_input' } : p,
  ),
  after: async (dom) => {
    await dom.sleep(800);
  },
};
export default s;
