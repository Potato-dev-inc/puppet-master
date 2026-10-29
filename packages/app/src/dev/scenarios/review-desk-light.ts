import { REVIEW_DESK_FULL } from '../review-desk-fixtures';
import type { HarnessScenario } from '../types';

// The same Review Desk in the light theme.
const s: HarnessScenario = {
  name: 'review-desk-light',
  route: 'cockpit',
  theme: 'light',
  reviewDesk: { desk: REVIEW_DESK_FULL },
};
export default s;
