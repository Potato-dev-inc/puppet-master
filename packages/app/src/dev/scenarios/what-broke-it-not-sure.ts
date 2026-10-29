import { NOT_SURE_RUNS } from '../what-broke-it-result-fixtures';
import type { HarnessScenario } from '../types';

// Ran out of runs with a narrowed range: the range line and Keep looking.
const s: HarnessScenario = {
  name: 'what-broke-it-not-sure',
  route: 'cockpit',
  whatBrokeItResult: { finding: NOT_SURE_RUNS },
};
export default s;
