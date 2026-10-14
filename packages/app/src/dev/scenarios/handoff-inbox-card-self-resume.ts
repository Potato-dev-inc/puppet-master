import { LIMIT_CARD_SELF_RESUME } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// Claude Code's own "continue automatically at usage limit" wait is on: the card says it will carry on by
// itself. Puppet Master leaves that wait exactly as it is.
const s: HarnessScenario = {
  name: 'handoff-inbox-card-self-resume',
  route: 'cockpit',
  questions: [LIMIT_CARD_SELF_RESUME],
};
export default s;
