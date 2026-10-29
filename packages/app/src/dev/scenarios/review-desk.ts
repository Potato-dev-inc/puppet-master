import { REVIEW_DESK_FULL } from '../review-desk-fixtures';
import type { HarnessScenario } from '../types';

// The Review Desk with five finished items: two high risk, one check first, two low risk, one group of helpers.
const s: HarnessScenario = {
  name: 'review-desk',
  route: 'cockpit',
  reviewDesk: { desk: REVIEW_DESK_FULL },
};
export default s;
