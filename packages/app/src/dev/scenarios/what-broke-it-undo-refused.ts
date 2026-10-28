import { CULPRIT, UNDO_PLAN_CONFLICT } from '../what-broke-it-result-fixtures';
import type { HarnessScenario } from '../types';

// A later turn touched the same lines: the undo confirm shows the refusal and nothing is written.
const s: HarnessScenario = {
  name: 'what-broke-it-undo-refused',
  route: 'cockpit',
  whatBrokeItResult: { finding: CULPRIT, undoPlan: UNDO_PLAN_CONFLICT },
};
export default s;
