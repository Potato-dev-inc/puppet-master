import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// Keep going: Claude Code ran out of usage. The Inbox card leads with "Continue on Codex" and keeps
// "Remind me at 14:05", "Stop here" and the plain "Other agent…" button.
const s: HarnessScenario = {
  name: 'keep-going-card',
  route: 'cockpit',
  questions: [LIMIT_CARD],
};
export default s;
