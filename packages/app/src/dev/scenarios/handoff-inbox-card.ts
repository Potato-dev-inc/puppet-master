import { LIMIT_CARD } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// Hand-off, detect-only: Claude Code ran out of usage. The Inbox card says when it can continue and offers
// "Remind me at 14:05" and "Stop here". No hand-off button yet; the card says that is coming later.
const s: HarnessScenario = {
  name: 'handoff-inbox-card',
  route: 'cockpit',
  questions: [LIMIT_CARD],
};
export default s;
