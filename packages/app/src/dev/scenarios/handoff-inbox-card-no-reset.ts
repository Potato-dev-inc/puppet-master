import { LIMIT_CARD_NO_RESET } from '../limit-fixtures';
import type { HarnessScenario } from '../types';

// The screen did not print a reset time: the card says so and offers a reminder in an hour.
const s: HarnessScenario = {
  name: 'handoff-inbox-card-no-reset',
  route: 'cockpit',
  questions: [LIMIT_CARD_NO_RESET],
};
export default s;
