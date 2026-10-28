import { SEVERAL } from '../what-broke-it-result-fixtures';
import type { HarnessScenario } from '../types';

// Two agents at the same time: two rows, no undo.
const s: HarnessScenario = {
  name: 'what-broke-it-several',
  route: 'cockpit',
  whatBrokeItResult: { finding: SEVERAL },
};
export default s;
