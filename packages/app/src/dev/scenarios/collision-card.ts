import { ALERTS } from '../collision-fixtures';
import type { HarnessScenario } from '../types';

// Collision Alert: Claude Code and Codex both changed lines 40-80 of src/auth.ts in separate copies. The Inbox card
// says so, names the one to finish first, and offers "Open Claude Code", "Open Codex" and "Ignore".
const s: HarnessScenario = {
  name: 'collision-card',
  route: 'cockpit',
  collision: { alerts: ALERTS },
  after: async (dom) => {
    await dom.waitFor('[data-collision-id]');
    await dom.sleep(400);
  },
};
export default s;
