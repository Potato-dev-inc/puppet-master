import { EXAMPLE_PANES, PANE_CLAUDE, PANE_CODEX } from '../fixtures';
import type { HarnessScenario } from '../types';

// Inbox with one high-risk approval card: details open, "Always allow" demoted behind More options.
const s: HarnessScenario = {
  name: 'approval-high',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) =>
    p.id === PANE_CLAUDE ? { ...p, status: 'idle' } : p.id === PANE_CODEX ? { ...p, status: 'waiting_input' } : p,
  ),
  after: async (dom) => {
    await dom.sleep(800);
    await dom.clickText('button', 'Show details');
    await dom.sleep(200);
  },
};
export default s;
