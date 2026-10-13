import type { HarnessScenario } from '../types';
import { EXAMPLE_PANES, PANE_CLAUDE } from '../fixtures';

// A pane started under an account shows its label in the header.
const s: HarnessScenario = {
  name: 'cockpit-pane-account',
  route: 'cockpit',
  select: PANE_CLAUDE,
  panes: EXAMPLE_PANES.map((p) => (p.id === PANE_CLAUDE ? { ...p, account: 'acc_work', account_label: 'Work (Max)' } : p)),
};
export default s;
