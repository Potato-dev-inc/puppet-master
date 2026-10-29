import { REVIEW_DESK_EMPTY } from '../review-desk-fixtures';
import type { HarnessScenario } from '../types';

// Nothing to review, and one source that could not be read (a plain note).
const s: HarnessScenario = {
  name: 'review-desk-empty',
  route: 'cockpit',
  reviewDesk: { desk: REVIEW_DESK_EMPTY },
};
export default s;
