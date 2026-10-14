import { LIMIT_REMINDER } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// The reminder the person asked for, at the reset time.
const s: HarnessScenario = {
  name: 'handoff-inbox-reminder',
  route: 'cockpit',
  questions: [LIMIT_REMINDER],
};
export default s;
