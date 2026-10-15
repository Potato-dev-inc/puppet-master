import { EXAMPLE_PANES } from '../fixtures';
import type { HarnessScenario } from '../types';

// Agents are open but none needs the person: the calm empty Inbox.
const s: HarnessScenario = {
  name: 'cockpit-inbox-clear',
  route: 'cockpit',
  panes: EXAMPLE_PANES.map((p) => ({ ...p, status: 'idle' as const })),
};
export default s;
