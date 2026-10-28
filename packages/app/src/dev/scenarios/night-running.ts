import { RUNNING_STATE } from '../night-fixtures';
import type { HarnessScenario } from '../types';

// A run is working: one task in progress, two in the queue. Start is off and says why.
const s: HarnessScenario = {
  name: 'night-running',
  route: 'cockpit',
  night: { state: RUNNING_STATE },
};
export default s;
